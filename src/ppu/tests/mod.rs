use super::*;

struct TestPPUBus {
    mem: [u8; 0x4000],
    read_log: Vec<u16>,
}

impl TestPPUBus {
    fn new() -> Self {
        Self {
            mem: [0; 0x4000],
            read_log: Vec::new(),
        }
    }
}

impl PPUBus for TestPPUBus {
    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        self.read_log.push(addr);
        self.mem[addr as usize]
    }

    fn ppu_write(&mut self, addr: u16, data: u8) {
        self.mem[(addr & 0x3FFF) as usize] = data;
    }
}

fn run_ppu_cycles(ppu: &mut PPU, bus: &mut TestPPUBus, cycles: usize) {
    for _ in 0..cycles {
        ppu.clock(bus);
    }
}

fn run_until_next_frame(ppu: &mut PPU, bus: &mut TestPPUBus) -> usize {
    let start_frame = ppu.frame();
    let mut cycles = 0;

    while ppu.frame() == start_frame {
        ppu.clock(bus);
        cycles += 1;
    }

    cycles
}

fn set_sprite(oam: &mut [u8; 256], index: usize, y: u8, tile: u8, attributes: u8, x: u8) {
    let base = index * 4;
    oam[base] = y;
    oam[base + 1] = tile;
    oam[base + 2] = attributes;
    oam[base + 3] = x;
}

#[test]
fn ppudata_write_uses_configured_increment() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2000, CTRL_VRAM_INCREMENT);
    ppu.cpu_write_register(&mut bus, 0x2006, 0x20);
    ppu.cpu_write_register(&mut bus, 0x2006, 0x00);
    ppu.cpu_write_register(&mut bus, 0x2007, 0x12);
    ppu.cpu_write_register(&mut bus, 0x2007, 0x34);

    assert_eq!(bus.mem[0x2000], 0x12);
    assert_eq!(bus.mem[0x2020], 0x34);
}

#[test]
fn writing_ppuctrl_with_nmi_enabled_should_assert_nmi_during_vblank() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.status = STATUS_VBLANK;

    ppu.cpu_write_register(&mut bus, 0x2000, CTRL_NMI_ENABLE);

    assert!(ppu.nmi_line(), "NMI line should be asserted during VBlank");
}

#[test]
fn writing_ppuctrl_with_nmi_disabled_should_not_assert_nmi_during_vblank() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.status = STATUS_VBLANK;

    ppu.cpu_write_register(&mut bus, 0x2000, 0x00);

    assert!(
        !ppu.nmi_line(),
        "NMI line should remain low when PPUCTRL bit 7 is clear"
    );
}

#[test]
fn reading_ppustatus_should_clear_vblank_flag() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.status = STATUS_VBLANK;
    ppu.open_bus = 0x1B;

    let status = ppu.cpu_read_register(&mut bus, 0x2002);

    assert_eq!(status, 0x9B);
    assert!(!ppu.in_vblank(), "reading PPUSTATUS should clear VBlank");
}

#[test]
fn reading_ppustatus_should_reset_write_toggle() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.write_latch = true;

    let _ = ppu.cpu_read_register(&mut bus, 0x2002);

    assert!(
        !ppu.write_latch,
        "reading PPUSTATUS should clear the write toggle"
    );
}

#[test]
fn clock_enters_vblank_and_asserts_nmi_line_when_enabled() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2000, CTRL_NMI_ENABLE);
    ppu.scanline = 241;
    ppu.cycles = 0;

    ppu.clock(&mut bus);
    assert!(!ppu.in_vblank(), "VBlank should remain low on dot 0");

    ppu.clock(&mut bus);

    assert!(ppu.in_vblank());
    assert!(ppu.nmi_line());
    assert_eq!(ppu.scanline(), 241);
}

#[test]
fn clock_clears_vblank_on_pre_render_scanline() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.status = STATUS_VBLANK | STATUS_SPRITE_ZERO_HIT | STATUS_SPRITE_OVERFLOW;
    ppu.scanline = 261;
    ppu.cycles = 0;

    ppu.clock(&mut bus);
    assert!(
        ppu.in_vblank(),
        "VBlank should remain set on pre-render dot 0"
    );

    ppu.clock(&mut bus);

    assert!(!ppu.in_vblank());
    assert_eq!(ppu.status & STATUS_SPRITE_ZERO_HIT, 0);
    assert_eq!(ppu.status & STATUS_SPRITE_OVERFLOW, 0);
}

#[test]
fn reading_ppustatus_on_the_last_pre_vblank_dot_suppresses_vblank_for_that_frame() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.scanline = 241;
    ppu.cycles = 0;

    let status = ppu.cpu_read_register(&mut bus, 0x2002);
    assert_eq!(status & STATUS_VBLANK, 0);

    ppu.clock(&mut bus);
    ppu.clock(&mut bus);

    assert!(
        !ppu.in_vblank(),
        "reading PPUSTATUS on the last pre-VBlank dot should suppress the flag"
    );
}

#[test]
fn reading_ppustatus_on_vblank_dot_one_reports_set_but_clears_and_suppresses_it() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.scanline = 241;
    ppu.cycles = 1;

    let status = ppu.cpu_read_register(&mut bus, 0x2002);

    assert_ne!(status & STATUS_VBLANK, 0);
    assert!(!ppu.in_vblank());

    ppu.clock(&mut bus);
    assert!(!ppu.in_vblank());
}

#[test]
fn reading_ppustatus_on_vblank_dot_two_also_suppresses_the_nmi_window() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.scanline = 241;
    ppu.cycles = 2;

    let status = ppu.cpu_read_register(&mut bus, 0x2002);

    assert_ne!(status & STATUS_VBLANK, 0);
    assert!(!ppu.in_vblank());

    ppu.clock(&mut bus);
    assert!(!ppu.in_vblank());
}

#[test]
fn clock_renders_background_pixels_across_tile_boundaries() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_BG | MASK_SHOW_BG_LEFTMOST);

    bus.mem[0x2000] = 0x01;
    bus.mem[0x2001] = 0x02;
    bus.mem[0x23C0] = 0x00;
    bus.mem[0x0010] = 0b1000_0000;
    bus.mem[0x0018] = 0x00;
    bus.mem[0x0020] = 0b1000_0000;
    bus.mem[0x0028] = 0x00;
    bus.mem[0x3F00] = 0x09;
    bus.mem[0x3F01] = 0x12;

    run_ppu_cycles(&mut ppu, &mut bus, 341 + 16);

    assert_eq!(ppu.bit_map[0], 0x12);
    assert_eq!(ppu.bit_map[1], 0x09);
    assert_eq!(ppu.bit_map[7], 0x09);
    assert_eq!(ppu.bit_map[8], 0x12);
}

#[test]
fn clock_uses_fine_x_scroll_for_background_pixels() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_BG | MASK_SHOW_BG_LEFTMOST);
    ppu.cpu_write_register(&mut bus, 0x2005, 0x03);
    ppu.cpu_write_register(&mut bus, 0x2005, 0x00);

    bus.mem[0x2000] = 0x01;
    bus.mem[0x23C0] = 0x00;
    bus.mem[0x0010] = 0b1111_0000;
    bus.mem[0x0018] = 0x00;
    bus.mem[0x3F00] = 0x09;
    bus.mem[0x3F01] = 0x12;

    run_ppu_cycles(&mut ppu, &mut bus, 341 + 2);

    assert_eq!(ppu.bit_map[0], 0x12);
    assert_eq!(ppu.bit_map[1], 0x09);
}

#[test]
fn odd_frame_skips_one_dot_when_rendering_is_enabled() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_BG);

    let initial_prerender = run_until_next_frame(&mut ppu, &mut bus);
    let odd_frame = run_until_next_frame(&mut ppu, &mut bus);
    let even_frame = run_until_next_frame(&mut ppu, &mut bus);

    assert_eq!(initial_prerender, 341);
    assert_eq!(odd_frame, 341 * 262 - 1);
    assert_eq!(even_frame, 341 * 262);
}

#[test]
fn odd_frame_does_not_skip_when_rendering_is_disabled() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    let initial_prerender = run_until_next_frame(&mut ppu, &mut bus);
    let first_frame = run_until_next_frame(&mut ppu, &mut bus);
    let second_frame = run_until_next_frame(&mut ppu, &mut bus);

    assert_eq!(initial_prerender, 341);
    assert_eq!(first_frame, 341 * 262);
    assert_eq!(second_frame, first_frame);
}

#[test]
fn clock_renders_sprite_pixels_on_the_target_scanline() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_SPRITES);
    set_sprite(&mut ppu.oam, 0, 0x00, 0x01, 0x00, 8);

    bus.mem[0x0010] = 0b1000_0000;
    bus.mem[0x0018] = 0x00;
    bus.mem[0x3F11] = 0x22;

    run_ppu_cycles(&mut ppu, &mut bus, 341 * 2 + 9);

    assert_eq!(ppu.bit_map[256 + 8], 0x22);
}

#[test]
fn sprite_priority_bit_keeps_opaque_background_in_front() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(
        &mut bus,
        0x2001,
        MASK_SHOW_BG | MASK_SHOW_SPRITES | MASK_SHOW_BG_LEFTMOST,
    );
    set_sprite(&mut ppu.oam, 0, 0x00, 0x01, 0x20, 8);

    bus.mem[0x2001] = 0x02;
    bus.mem[0x23C0] = 0x00;
    bus.mem[0x0021] = 0b1000_0000;
    bus.mem[0x0029] = 0x00;
    bus.mem[0x0010] = 0b1000_0000;
    bus.mem[0x0018] = 0x00;
    bus.mem[0x3F01] = 0x12;
    bus.mem[0x3F11] = 0x22;

    run_ppu_cycles(&mut ppu, &mut bus, 341 * 2 + 9);

    assert_eq!(ppu.bit_map[256 + 8], 0x12);
}

#[test]
fn sprite_zero_hit_sets_when_sprite_zero_overlaps_background() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(
        &mut bus,
        0x2001,
        MASK_SHOW_BG | MASK_SHOW_SPRITES | MASK_SHOW_BG_LEFTMOST,
    );
    set_sprite(&mut ppu.oam, 0, 0x00, 0x01, 0x00, 8);

    bus.mem[0x2001] = 0x02;
    bus.mem[0x23C0] = 0x00;
    bus.mem[0x0021] = 0b1000_0000;
    bus.mem[0x0029] = 0x00;
    bus.mem[0x0010] = 0b1000_0000;
    bus.mem[0x0018] = 0x00;
    bus.mem[0x3F01] = 0x12;
    bus.mem[0x3F11] = 0x22;

    run_ppu_cycles(&mut ppu, &mut bus, 341 * 2 + 9);

    assert_ne!(ppu.status & STATUS_SPRITE_ZERO_HIT, 0);
    assert_eq!(ppu.bit_map[256 + 8], 0x22);
}

#[test]
fn sprite_overflow_sets_when_more_than_eight_sprites_share_a_scanline() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_SPRITES);

    for index in 0..9 {
        set_sprite(&mut ppu.oam, index, 0x00, 0x01, 0x00, (index * 8) as u8);
    }

    run_ppu_cycles(&mut ppu, &mut bus, 341 * 2);

    assert_ne!(ppu.status & STATUS_SPRITE_OVERFLOW, 0);
}

#[test]
fn eight_by_sixteen_sprites_fetch_the_second_tile_for_bottom_half_rows() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2000, CTRL_SPRITE_SIZE);
    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_SPRITES);
    set_sprite(&mut ppu.oam, 0, 0x00, 0x02, 0x00, 8);

    bus.mem[0x0030] = 0b1000_0000;
    bus.mem[0x0038] = 0x00;
    bus.mem[0x3F11] = 0x22;

    run_ppu_cycles(&mut ppu, &mut bus, 341 * 10 + 9);

    assert_eq!(ppu.bit_map[9 * 256 + 8], 0x22);
}

#[test]
fn horizontally_flipped_sprites_reverse_pattern_bits() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_SPRITES);
    set_sprite(&mut ppu.oam, 0, 0x00, 0x01, 0x40, 8);

    bus.mem[0x0010] = 0b0000_0001;
    bus.mem[0x0018] = 0x00;
    bus.mem[0x3F11] = 0x22;

    run_ppu_cycles(&mut ppu, &mut bus, 341 * 2 + 9);

    assert_eq!(ppu.bit_map[256 + 8], 0x22);
}

#[test]
fn vertically_flipped_sprites_fetch_rows_from_the_bottom_up() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_SPRITES);
    set_sprite(&mut ppu.oam, 0, 0x00, 0x01, 0x80, 8);

    bus.mem[0x0017] = 0b1000_0000;
    bus.mem[0x001F] = 0x00;
    bus.mem[0x3F11] = 0x22;

    run_ppu_cycles(&mut ppu, &mut bus, 341 * 2 + 9);

    assert_eq!(ppu.bit_map[256 + 8], 0x22);
}

#[test]
fn grayscale_mask_applies_to_rendered_palette_output() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(
        &mut bus,
        0x2001,
        MASK_GRAYSCALE | MASK_SHOW_BG | MASK_SHOW_BG_LEFTMOST,
    );

    bus.mem[0x2000] = 0x01;
    bus.mem[0x23C0] = 0x00;
    bus.mem[0x0010] = 0b1000_0000;
    bus.mem[0x0018] = 0x00;
    bus.mem[0x3F01] = 0x2D;

    run_ppu_cycles(&mut ppu, &mut bus, 341 + 1);

    assert_eq!(ppu.bit_map[0], 0x20);
}

#[test]
fn grayscale_mask_applies_to_palette_ram_reads() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_GRAYSCALE);
    ppu.cpu_write_register(&mut bus, 0x2006, 0x3F);
    ppu.cpu_write_register(&mut bus, 0x2006, 0x01);
    bus.mem[0x3F01] = 0x2D;

    assert_eq!(ppu.cpu_read_register(&mut bus, 0x2007), 0x20);
}

#[test]
fn ppudata_write_during_rendering_increments_scroll_x_and_y_instead_of_linearly() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_BG);
    ppu.scanline = 0;
    ppu.loopy_v = 0x23A0;
    ppu.vram_addr = 0x23A0;

    ppu.cpu_write_register(&mut bus, 0x2007, 0x12);

    assert_eq!(bus.mem[0x23A0], 0x12);
    assert_eq!(ppu.loopy_v, 0x33A1);
    assert_eq!(ppu.vram_addr, 0x33A1);
}

#[test]
fn ppudata_rendering_increment_ignores_ppuctrl_increment_bit() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2000, CTRL_VRAM_INCREMENT);
    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_BG);
    ppu.scanline = 0;
    ppu.loopy_v = 0x0000;
    ppu.vram_addr = 0x0000;

    ppu.cpu_write_register(&mut bus, 0x2007, 0x34);

    assert_eq!(ppu.loopy_v, 0x1001);
}

#[test]
fn increment_x_should_toggle_horizontal_nametable_when_coarse_x_wraps_from_tile_31() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_BG);
    ppu.loopy_v = 0x5A3F;
    ppu.vram_addr = 0x5A3F;

    ppu.increment_x();

    assert_eq!((ppu.loopy_v, ppu.vram_addr), (0x5E20, 0x5E20));
}

#[test]
fn increment_y_should_apply_coarse_y_29_and_31_wrap_rules_when_fine_y_is_7() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_BG);

    ppu.loopy_v = 0x77A3;
    ppu.vram_addr = 0x77A3;
    ppu.increment_y();
    assert_eq!((ppu.loopy_v, ppu.vram_addr), (0x0C03, 0x0C03));

    ppu.loopy_v = 0x7FE5;
    ppu.vram_addr = 0x7FE5;
    ppu.increment_y();
    assert_eq!((ppu.loopy_v, ppu.vram_addr), (0x0C05, 0x0C05));
}

#[test]
fn frame_pixels_only_exposes_the_visible_256_by_240_region() {
    let mut ppu = PPU::new();
    ppu.bit_map[0] = 0x01;
    ppu.bit_map[FRAME_WIDTH * FRAME_HEIGHT - 1] = 0x02;

    let pixels = ppu.frame_pixels();

    assert_eq!(pixels.len(), FRAME_WIDTH * FRAME_HEIGHT);
    assert_eq!(pixels[0], 0x01);
    assert_eq!(pixels[FRAME_WIDTH * FRAME_HEIGHT - 1], 0x02);
}

#[test]
fn frame_rgb_converts_palette_indices_to_rgb_bytes() {
    let mut ppu = PPU::new();
    ppu.bit_map[0] = 0x00;
    ppu.bit_map[1] = 0x01;
    ppu.bit_map[2] = 0x21;

    let rgb = ppu.frame_rgb();

    assert_eq!(rgb.len(), FRAME_WIDTH * FRAME_HEIGHT * 3);
    assert_eq!(&rgb[..9], &[84, 84, 84, 0, 30, 116, 76, 154, 236]);
}

#[test]
fn interleaved_ppuaddr_and_ppuscroll_writes_follow_shared_write_toggle_rules() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2006, 0x04);
    assert_eq!(ppu.loopy_t, 0x0400);
    assert!(ppu.write_latch);

    ppu.cpu_write_register(&mut bus, 0x2005, 0x3E);
    assert_eq!(ppu.loopy_t, 0x64E0);
    assert!(!ppu.write_latch);

    ppu.cpu_write_register(&mut bus, 0x2005, 0x7D);
    assert_eq!(ppu.loopy_t, 0x64EF);
    assert_eq!(ppu.fine_x, 0x05);
    assert!(ppu.write_latch);

    ppu.cpu_write_register(&mut bus, 0x2006, 0xEF);
    assert_eq!(ppu.loopy_t, 0x64EF);
    assert_eq!(ppu.loopy_v, 0x64EF);
    assert_eq!(ppu.vram_addr, 0x64EF);
    assert!(!ppu.write_latch);
}

#[test]
fn second_ppuaddr_write_updates_current_vram_address_immediately_even_while_rendering() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_BG);
    ppu.scanline = 120;
    ppu.cycles = 100;
    ppu.loopy_v = 0x2000;
    ppu.vram_addr = 0x2000;

    ppu.cpu_write_register(&mut bus, 0x2006, 0x24);
    assert_eq!(ppu.loopy_v, 0x2000);

    ppu.cpu_write_register(&mut bus, 0x2006, 0x80);

    assert_eq!(ppu.loopy_t, 0x2480);
    assert_eq!(ppu.loopy_v, 0x2480);
    assert_eq!(ppu.vram_addr, 0x2480);
}

#[test]
fn reading_oamdata_masks_unimplemented_attribute_bits() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.oam_addr = 0x02;
    ppu.oam[0x02] = 0xFF;

    assert_eq!(ppu.cpu_read_register(&mut bus, 0x2004), 0xE3);
}

#[test]
fn reading_oamdata_during_sprite_clear_phase_returns_ff() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_BG);
    ppu.scanline = 0;
    ppu.cycles = 10;
    ppu.oam_addr = 0x00;
    ppu.oam[0x00] = 0x12;

    assert_eq!(ppu.cpu_read_register(&mut bus, 0x2004), 0xFF);
}

#[test]
fn writing_oamdata_during_rendering_only_advances_oamaddr_by_four() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_SPRITES);
    ppu.scanline = 20;
    ppu.cycles = 100;
    ppu.oam_addr = 0x05;
    ppu.oam[0x05] = 0xAA;

    ppu.cpu_write_register(&mut bus, 0x2004, 0x33);

    assert_eq!(ppu.oam[0x05], 0xAA);
    assert_eq!(ppu.oam_addr, 0x09);
}

#[test]
fn sprites_can_render_on_the_first_visible_scanline() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_SPRITES);
    set_sprite(&mut ppu.oam, 0, 0xFF, 0x01, 0x00, 8);

    bus.mem[0x0010] = 0b1000_0000;
    bus.mem[0x0018] = 0x00;
    bus.mem[0x3F11] = 0x22;

    run_ppu_cycles(&mut ppu, &mut bus, 341 + 9);

    assert_eq!(ppu.bit_map[8], 0x22);
}

#[test]
fn sprite_pattern_fetches_happen_during_sprite_fetch_phase_not_during_evaluation() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2000, CTRL_SPRITE_TABLE);
    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_SPRITES);
    set_sprite(&mut ppu.oam, 0, 0xFF, 0x01, 0x00, 8);
    ppu.scanline = 261;
    ppu.cycles = 256;

    ppu.clock(&mut bus);
    assert!(
        !bus.read_log
            .iter()
            .any(|&addr| addr == 0x1010 || addr == 0x1018),
        "sprite evaluation should not fetch pattern bytes immediately"
    );

    for _ in 0..7 {
        ppu.clock(&mut bus);
    }

    assert!(bus.read_log.contains(&0x1010));
    assert!(bus.read_log.contains(&0x1018));
}

#[test]
fn sprite_overflow_still_sets_when_only_background_rendering_is_enabled() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_BG);

    for index in 0..9 {
        set_sprite(&mut ppu.oam, index, 0x00, 0x01, 0x00, (index * 8) as u8);
    }

    run_ppu_cycles(&mut ppu, &mut bus, 341 * 2);

    assert_ne!(ppu.status & STATUS_SPRITE_OVERFLOW, 0);
}

#[test]
fn sprite_overflow_bug_can_raise_false_positive_from_non_y_byte() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_SPRITES);

    for index in 0..8 {
        set_sprite(&mut ppu.oam, index, 0x00, 0x01, 0x00, (index * 8) as u8);
    }

    set_sprite(&mut ppu.oam, 8, 0x20, 0x00, 0x00, 0x00);
    set_sprite(&mut ppu.oam, 9, 0x20, 0x01, 0x20, 0x20);

    run_ppu_cycles(&mut ppu, &mut bus, 341 * 2);

    assert_ne!(ppu.status & STATUS_SPRITE_OVERFLOW, 0);
}

#[test]
fn sprite_overflow_bug_can_miss_a_real_ninth_sprite() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2001, MASK_SHOW_SPRITES);

    for index in 0..8 {
        set_sprite(&mut ppu.oam, index, 0x00, 0x01, 0x00, (index * 8) as u8);
    }

    for index in 8..64 {
        set_sprite(&mut ppu.oam, index, 0x20, 0x20, 0x20, 0x20);
    }
    set_sprite(&mut ppu.oam, 9, 0x00, 0x20, 0x20, 0x20);

    run_ppu_cycles(&mut ppu, &mut bus, 341 * 2);

    assert_eq!(ppu.status & STATUS_SPRITE_OVERFLOW, 0);
}
