use super::*;

const CHR_HALF_BANK_LEN: usize = 0x1000;
const CHR_BANK_LEN_1K: usize = 0x0400;

fn make_ines(prg_banks: u8, chr_banks: u8, flags6: u8, prg_fill: u8, chr_fill: u8) -> Vec<u8> {
    let mut rom = vec![0; INES_HEADER_LEN];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = prg_banks;
    rom[5] = chr_banks;
    rom[6] = flags6;

    rom.extend(std::iter::repeat_n(
        prg_fill,
        prg_banks as usize * PRG_BANK_LEN,
    ));
    rom.extend(std::iter::repeat_n(
        chr_fill,
        chr_banks as usize * CHR_BANK_LEN,
    ));
    rom
}

fn make_ines_with_prg(prg_rom: &[u8], chr_banks: u8, flags6: u8, chr_fill: u8) -> Vec<u8> {
    assert_eq!(prg_rom.len() % PRG_BANK_LEN, 0);

    let mut rom = vec![0; INES_HEADER_LEN];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = (prg_rom.len() / PRG_BANK_LEN) as u8;
    rom[5] = chr_banks;
    rom[6] = flags6;
    rom.extend_from_slice(prg_rom);
    rom.extend(std::iter::repeat_n(
        chr_fill,
        chr_banks as usize * CHR_BANK_LEN,
    ));
    rom
}

fn make_ines_with_prg_chr(prg_rom: &[u8], chr_rom: &[u8], flags6: u8) -> Vec<u8> {
    assert_eq!(prg_rom.len() % PRG_BANK_LEN, 0);
    assert_eq!(chr_rom.len() % CHR_BANK_LEN, 0);

    let mut rom = vec![0; INES_HEADER_LEN];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = (prg_rom.len() / PRG_BANK_LEN) as u8;
    rom[5] = (chr_rom.len() / CHR_BANK_LEN) as u8;
    rom[6] = flags6;
    rom.extend_from_slice(prg_rom);
    rom.extend_from_slice(chr_rom);
    rom
}

fn write_mmc1_register(cartridge: &mut Cartridge, addr: u16, value: u8) {
    for bit in 0..5 {
        let serial_bit = (value >> bit) & 0x01;
        assert!(cartridge.cpu_write(addr, serial_bit));
    }
}

fn make_ines_with_flags(prg_rom: &[u8], chr_rom: &[u8], flags6: u8) -> Vec<u8> {
    assert_eq!(prg_rom.len() % PRG_BANK_LEN, 0);
    assert_eq!(chr_rom.len() % CHR_BANK_LEN, 0);

    let mut rom = vec![0; INES_HEADER_LEN];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = (prg_rom.len() / PRG_BANK_LEN) as u8;
    rom[5] = (chr_rom.len() / CHR_BANK_LEN) as u8;
    rom[6] = flags6;
    rom.extend_from_slice(prg_rom);
    rom.extend_from_slice(chr_rom);
    rom
}

fn make_ines_with_tv_bytes(flags9: u8, flags10: u8, tail: [u8; 5]) -> Vec<u8> {
    let mut rom = make_ines(1, 1, 0x00, 0xEA, 0x55);
    rom[9] = flags9;
    rom[10] = flags10;
    rom[11..16].copy_from_slice(&tail);
    rom
}

#[test]
fn parses_ines_header_and_maps_nrom_prg() {
    let mut rom = make_ines(1, 1, 0x01, 0xEA, 0x55);
    let prg_start = INES_HEADER_LEN;
    rom[prg_start] = 0x78;
    rom[prg_start + 0x3FFF] = 0x4C;

    let mut cartridge = Cartridge::from_ines(&rom).expect("valid NROM should parse");

    assert_eq!(cartridge.mirroring(), Mirroring::Vertical);
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x78));
    assert_eq!(cartridge.cpu_read(0xBFFF), Some(0x4C));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x78));
}

#[test]
fn allocates_chr_ram_when_chr_banks_are_zero() {
    let rom = make_ines(1, 0, 0x00, 0xEA, 0x00);
    let mut cartridge = Cartridge::from_ines(&rom).expect("CHR RAM cartridge should parse");

    assert_eq!(cartridge.ppu_read(0x000A), Some(0x00));
    assert!(cartridge.ppu_write(0x000A, 0x9C));
    assert_eq!(cartridge.ppu_read(0x000A), Some(0x9C));
}

#[test]
fn rejects_unsupported_mapper() {
    // Mapper 14 is not implemented (flags6 >> 4 = 14, with SRAM flag set)
    let rom = make_ines(1, 1, 0xE2, 0x00, 0x00);

    let err = match Cartridge::from_ines(&rom) {
        Ok(_) => panic!("mapper 14 should be rejected"),
        Err(err) => err,
    };

    assert_eq!(err, CartridgeError::UnsupportedMapper(14));
}

#[test]
fn parses_ines_header_and_maps_uxrom_prg_banks() {
    let mut prg_rom = Vec::with_capacity(4 * PRG_BANK_LEN);
    for bank in 0..4_u8 {
        prg_rom.extend(std::iter::repeat_n(bank, PRG_BANK_LEN));
    }
    let rom = make_ines_with_prg(&prg_rom, 1, 0x20, 0xAA);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid UxROM should parse");

    assert_eq!(cartridge.cpu_read(0x8000), Some(0x00));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x03));

    assert!(cartridge.cpu_write(0x8000, 0x02));

    assert_eq!(cartridge.cpu_read(0x8000), Some(0x02));
    assert_eq!(cartridge.cpu_read(0xBFFF), Some(0x02));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x03));
    assert_eq!(cartridge.cpu_read(0xFFFF), Some(0x03));
}

#[test]
fn cnrom_switches_chr_banks_with_cpu_writes() {
    let prg_rom = vec![0xEA; 2 * PRG_BANK_LEN];
    let mut chr_rom = Vec::with_capacity(3 * CHR_BANK_LEN);
    chr_rom.extend(std::iter::repeat_n(0x10, CHR_BANK_LEN));
    chr_rom.extend(std::iter::repeat_n(0x20, CHR_BANK_LEN));
    chr_rom.extend(std::iter::repeat_n(0x30, CHR_BANK_LEN));
    let rom = make_ines_with_prg_chr(&prg_rom, &chr_rom, 0x30);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid CNROM should parse");

    assert_eq!(cartridge.ppu_read(0x0000), Some(0x10));
    assert_eq!(cartridge.ppu_read(0x1FFF), Some(0x10));

    assert!(cartridge.cpu_write(0x8000, 0x02));
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x30));
    assert_eq!(cartridge.ppu_read(0x1FFF), Some(0x30));

    // CNROM bank select lines are wider than some carts; values should wrap to available banks.
    assert!(cartridge.cpu_write(0xFFFF, 0x07));
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x20));
}

#[test]
fn anrom_switches_32k_prg_bank_with_low_nibble() {
    let mut prg_rom = Vec::with_capacity(4 * 0x8000);
    for bank in 0..4_u8 {
        prg_rom.extend(std::iter::repeat_n(bank, 0x8000));
    }
    let rom = make_ines_with_prg(&prg_rom, 0, 0x70, 0x00);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid ANROM should parse");

    assert_eq!(cartridge.cpu_read(0x8000), Some(0x00));
    assert_eq!(cartridge.cpu_read(0xFFFF), Some(0x00));

    assert!(cartridge.cpu_write(0x8000, 0x13));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x03));
    assert_eq!(cartridge.cpu_read(0xFFFF), Some(0x03));
}

#[test]
fn anrom_uses_bit4_for_one_screen_mirroring_select() {
    let prg_rom = vec![0xEA; 2 * PRG_BANK_LEN];
    let rom = make_ines_with_prg(&prg_rom, 0, 0x70, 0x00);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid ANROM should parse");

    assert_eq!(cartridge.mirroring(), Mirroring::SPAGE0);

    assert!(cartridge.cpu_write(0x8000, 0x10));
    assert_eq!(cartridge.mirroring(), Mirroring::SPAGE1);

    assert!(cartridge.cpu_write(0x8000, 0x00));
    assert_eq!(cartridge.mirroring(), Mirroring::SPAGE0);
}

#[test]
fn uxrom_bank_select_wraps_when_value_exceeds_bank_count() {
    let mut prg_rom = Vec::with_capacity(2 * PRG_BANK_LEN);
    prg_rom.extend(std::iter::repeat_n(0x11, PRG_BANK_LEN));
    prg_rom.extend(std::iter::repeat_n(0x22, PRG_BANK_LEN));
    let rom = make_ines_with_prg(&prg_rom, 1, 0x20, 0xAA);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid UxROM should parse");

    assert!(cartridge.cpu_write(0x8000, 0x07));

    assert_eq!(cartridge.cpu_read(0x8000), Some(0x22));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x22));
}

#[test]
fn uxrom_allocates_chr_ram_when_chr_banks_are_zero() {
    let rom = make_ines(2, 0, 0x20, 0xEA, 0x00);
    let mut cartridge = Cartridge::from_ines(&rom).expect("CHR RAM UxROM should parse");

    assert_eq!(cartridge.ppu_read(0x0010), Some(0x00));
    assert!(cartridge.ppu_write(0x0010, 0x5C));
    assert_eq!(cartridge.ppu_read(0x0010), Some(0x5C));
}

#[test]
fn mmc1_switches_lower_prg_bank_in_fix_last_bank_mode() {
    let mut prg_rom = Vec::with_capacity(4 * PRG_BANK_LEN);
    for bank in 0..4_u8 {
        prg_rom.extend(std::iter::repeat_n(bank, PRG_BANK_LEN));
    }
    let rom = make_ines_with_prg(&prg_rom, 0, 0x10, 0x00);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid MMC1 should parse");

    write_mmc1_register(&mut cartridge, 0xE000, 0x02);

    assert_eq!(cartridge.cpu_read(0x8000), Some(0x02));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x03));
}

#[test]
fn mmc1_control_register_updates_mirroring() {
    let rom = make_ines(2, 0, 0x10, 0xEA, 0x00);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid MMC1 should parse");

    assert_eq!(cartridge.mirroring(), Mirroring::SPAGE0);

    write_mmc1_register(&mut cartridge, 0x8000, 0x03);
    assert_eq!(cartridge.mirroring(), Mirroring::Horizontal);

    write_mmc1_register(&mut cartridge, 0x8000, 0x02);
    assert_eq!(cartridge.mirroring(), Mirroring::Vertical);
}

#[test]
fn mmc1_switches_chr_in_4k_mode() {
    let prg_rom = vec![0xEA; 2 * PRG_BANK_LEN];
    let mut chr_rom = Vec::with_capacity(2 * CHR_BANK_LEN);
    chr_rom.extend(std::iter::repeat_n(0x11, CHR_HALF_BANK_LEN));
    chr_rom.extend(std::iter::repeat_n(0x22, CHR_HALF_BANK_LEN));
    chr_rom.extend(std::iter::repeat_n(0x33, CHR_HALF_BANK_LEN));
    chr_rom.extend(std::iter::repeat_n(0x44, CHR_HALF_BANK_LEN));
    let rom = make_ines_with_prg_chr(&prg_rom, &chr_rom, 0x10);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid MMC1 should parse");

    write_mmc1_register(&mut cartridge, 0x8000, 0x10);
    write_mmc1_register(&mut cartridge, 0xA000, 0x01);
    write_mmc1_register(&mut cartridge, 0xC000, 0x03);

    assert_eq!(cartridge.ppu_read(0x0000), Some(0x22));
    assert_eq!(cartridge.ppu_read(0x1000), Some(0x44));
}

#[test]
fn mmc3_switches_prg_banks_and_respects_prg_mode() {
    let mut prg_rom = Vec::with_capacity(4 * PRG_BANK_LEN);
    for bank in 0..8_u8 {
        prg_rom.extend(std::iter::repeat_n(bank, PRG_BANK_LEN / 2));
    }
    let chr_rom = vec![0xAA; CHR_BANK_LEN];
    let rom = make_ines_with_flags(&prg_rom, &chr_rom, 0x40);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid MMC3 should parse");

    assert_eq!(cartridge.cpu_read(0x8000), Some(0x00));
    assert_eq!(cartridge.cpu_read(0xA000), Some(0x01));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x06));
    assert_eq!(cartridge.cpu_read(0xE000), Some(0x07));

    assert!(cartridge.cpu_write(0x8000, 0x06));
    assert!(cartridge.cpu_write(0x8001, 0x03));
    assert!(cartridge.cpu_write(0x8000, 0x07));
    assert!(cartridge.cpu_write(0x8001, 0x04));

    assert_eq!(cartridge.cpu_read(0x8000), Some(0x03));
    assert_eq!(cartridge.cpu_read(0xA000), Some(0x04));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x06));

    assert!(cartridge.cpu_write(0x8000, 0x46));
    assert!(cartridge.cpu_write(0x8001, 0x02));

    assert_eq!(cartridge.cpu_read(0x8000), Some(0x06));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x02));
}

#[test]
fn ines1_clean_header_can_request_ntsc_timing() {
    let rom = make_ines_with_tv_bytes(0x00, 0x00, [0; 5]);
    let cartridge = Cartridge::from_ines(&rom).expect("valid NROM should parse");

    assert_eq!(cartridge.tv_system(), TVSystem::NTSC);
}

#[test]
fn ines1_byte9_pal_flag_selects_pal_timing() {
    let rom = make_ines_with_tv_bytes(0x01, 0x00, [0; 5]);
    let cartridge = Cartridge::from_ines(&rom).expect("valid NROM should parse");

    assert_eq!(cartridge.tv_system(), TVSystem::PAL);
}

#[test]
fn ines1_dirty_extension_bytes_fall_back_to_ntsc_timing() {
    let rom = make_ines_with_tv_bytes(0x01, 0x00, [0x44, 0x69, 0x73, 0x6B, 0x44]);
    let cartridge = Cartridge::from_ines(&rom).expect("valid NROM should parse");

    assert_eq!(cartridge.tv_system(), TVSystem::NTSC);
}

#[test]
fn mmc3_starts_with_linear_chr_ram_and_first_two_prg_banks_mapped() {
    let mut prg_rom = Vec::with_capacity(4 * PRG_BANK_LEN);
    for bank in 0..8_u8 {
        prg_rom.extend(std::iter::repeat_n(bank, PRG_BANK_LEN / 2));
    }
    let rom = make_ines_with_flags(&prg_rom, &[], 0x40);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid MMC3 should parse");

    assert_eq!(cartridge.cpu_read(0x8000), Some(0x00));
    assert_eq!(cartridge.cpu_read(0xA000), Some(0x01));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x06));
    assert_eq!(cartridge.cpu_read(0xE000), Some(0x07));

    for (addr, value) in [
        (0x0000, 0x10),
        (0x0400, 0x11),
        (0x0800, 0x12),
        (0x0C00, 0x13),
        (0x1000, 0x14),
        (0x1400, 0x15),
        (0x1800, 0x16),
        (0x1C00, 0x17),
    ] {
        assert!(cartridge.ppu_write(addr, value));
        assert_eq!(cartridge.ppu_read(addr), Some(value));
    }
}

#[test]
fn mmc3_switches_chr_banks_in_1k_and_2k_regions() {
    let prg_rom = vec![0xEA; 2 * PRG_BANK_LEN];
    let mut chr_rom = Vec::with_capacity(CHR_BANK_LEN);
    for bank in 0..8_u8 {
        chr_rom.extend(std::iter::repeat_n(bank, CHR_BANK_LEN_1K));
    }
    let rom = make_ines_with_flags(&prg_rom, &chr_rom, 0x40);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid MMC3 should parse");

    assert!(cartridge.cpu_write(0x8000, 0x00));
    assert!(cartridge.cpu_write(0x8001, 0x02));
    assert!(cartridge.cpu_write(0x8000, 0x01));
    assert!(cartridge.cpu_write(0x8001, 0x04));
    assert!(cartridge.cpu_write(0x8000, 0x02));
    assert!(cartridge.cpu_write(0x8001, 0x06));
    assert!(cartridge.cpu_write(0x8000, 0x03));
    assert!(cartridge.cpu_write(0x8001, 0x07));

    assert_eq!(cartridge.ppu_read(0x0000), Some(0x02));
    assert_eq!(cartridge.ppu_read(0x0400), Some(0x03));
    assert_eq!(cartridge.ppu_read(0x0800), Some(0x04));
    assert_eq!(cartridge.ppu_read(0x0C00), Some(0x05));
    assert_eq!(cartridge.ppu_read(0x1000), Some(0x06));
    assert_eq!(cartridge.ppu_read(0x1400), Some(0x07));

    assert!(cartridge.cpu_write(0x8000, 0x80 | 0x02));
    assert!(cartridge.cpu_write(0x8001, 0x01));

    assert_eq!(cartridge.ppu_read(0x0000), Some(0x01));
    assert_eq!(cartridge.ppu_read(0x1000), Some(0x02));
    assert_eq!(cartridge.ppu_read(0x1400), Some(0x03));
}

#[test]
fn mmc3_updates_mirroring_and_raises_irq_on_filtered_a12_rises() {
    let prg_rom = vec![0xEA; 2 * PRG_BANK_LEN];
    let chr_rom = vec![0x55; CHR_BANK_LEN];
    let rom = make_ines_with_flags(&prg_rom, &chr_rom, 0x40);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid MMC3 should parse");

    assert_eq!(cartridge.mirroring(), Mirroring::Horizontal);
    assert!(cartridge.cpu_write(0xA000, 0x00));
    assert_eq!(cartridge.mirroring(), Mirroring::Vertical);
    assert!(cartridge.cpu_write(0xA000, 0x01));
    assert_eq!(cartridge.mirroring(), Mirroring::Horizontal);

    assert!(cartridge.cpu_write(0xC000, 0x01));
    assert!(cartridge.cpu_write(0xC001, 0x00));
    assert!(cartridge.cpu_write(0xE001, 0x00));

    for _ in 0..3 {
        cartridge.check_a12(0x0FFF, 0);
    }
    cartridge.check_a12(0x1000, 10);
    assert!(!cartridge.irq_line());

    for _ in 0..3 {
        cartridge.check_a12(0x0FFF, 16);
    }
    cartridge.check_a12(0x1000, 367);
    assert!(cartridge.irq_line());

    assert!(cartridge.cpu_write(0xE000, 0x00));
    assert!(!cartridge.irq_line());
}

#[test]
fn mmc3_counts_a12_rises_after_two_low_fetches_like_background_pattern_fetches() {
    let prg_rom = vec![0xEA; 2 * PRG_BANK_LEN];
    let chr_rom = vec![0x55; CHR_BANK_LEN];
    let rom = make_ines_with_flags(&prg_rom, &chr_rom, 0x40);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid MMC3 should parse");

    assert!(cartridge.cpu_write(0xC000, 0x00));
    assert!(cartridge.cpu_write(0xC001, 0x00));
    assert!(cartridge.cpu_write(0xE001, 0x00));

    cartridge.check_a12(0x2000, 0);
    cartridge.check_a12(0x23C0, 2);
    cartridge.check_a12(0x1000, 10);

    assert!(cartridge.irq_line());
}

#[test]
fn mmc3_filters_short_a12_pulses_with_ppu_cycle_timing() {
    let prg_rom = vec![0xEA; 2 * PRG_BANK_LEN];
    let chr_rom = vec![0x55; CHR_BANK_LEN];
    let rom = make_ines_with_flags(&prg_rom, &chr_rom, 0x40);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid MMC3 should parse");

    assert!(cartridge.cpu_write(0xC000, 0x00));
    assert!(cartridge.cpu_write(0xC001, 0x00));
    assert!(cartridge.cpu_write(0xE001, 0x00));

    cartridge.check_a12(0x1000, 0);
    cartridge.check_a12(0x0000, 4);
    cartridge.check_a12(0x1000, 6);
    assert!(
        !cartridge.irq_line(),
        "short low periods between sprite fetches should not clock MMC3 IRQs"
    );

    cartridge.check_a12(0x0000, 12);
    cartridge.check_a12(0x1000, 22);
    assert!(cartridge.irq_line());
}

#[test]
fn mmc3_filters_eight_cycle_low_periods_between_sparse_sprite_fetches() {
    let prg_rom = vec![0xEA; 2 * PRG_BANK_LEN];
    let chr_rom = vec![0x55; CHR_BANK_LEN];
    let rom = make_ines_with_flags(&prg_rom, &chr_rom, 0x40);
    let mut cartridge = Cartridge::from_ines(&rom).expect("valid MMC3 should parse");

    assert!(cartridge.cpu_write(0xC000, 0x00));
    assert!(cartridge.cpu_write(0xC001, 0x00));
    assert!(cartridge.cpu_write(0xE001, 0x00));

    cartridge.check_a12(0x1000, 0);
    cartridge.check_a12(0x0000, 4);
    cartridge.check_a12(0x1000, 12);
    assert!(
        !cartridge.irq_line(),
        "an eight-dot low window between sprite fetch groups should stay filtered"
    );

    cartridge.check_a12(0x0000, 16);
    cartridge.check_a12(0x1000, 26);
    assert!(cartridge.irq_line());
}

fn make_ines_mapper(prg_rom: &[u8], chr_rom: &[u8], mapper_id: u8) -> Vec<u8> {
    assert_eq!(prg_rom.len() % PRG_BANK_LEN, 0);
    assert_eq!(chr_rom.len() % CHR_BANK_LEN, 0);

    let mut rom = vec![0; INES_HEADER_LEN];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = (prg_rom.len() / PRG_BANK_LEN) as u8;
    rom[5] = (chr_rom.len() / CHR_BANK_LEN) as u8;
    rom[6] = ((mapper_id & 0x0F) << 4) | 0x01;
    rom[7] = mapper_id & 0xF0;
    rom.extend_from_slice(prg_rom);
    rom.extend_from_slice(chr_rom);
    rom
}

fn tagged_prg_rom(bank_count: usize, bank_len: usize) -> Vec<u8> {
    let mut rom = Vec::with_capacity(bank_count * bank_len);
    for bank in 0..bank_count {
        rom.extend(std::iter::repeat_n((bank % 0x100) as u8, bank_len));
    }
    rom
}

fn tagged_chr_rom(bank_count: usize, bank_len: usize) -> Vec<u8> {
    tagged_prg_rom(bank_count, bank_len)
}

#[test]
fn mapper37_offsets_mmc3_banks_with_multicart_register() {
    let prg_rom = tagged_prg_rom(32, 0x2000);
    let chr_rom = tagged_chr_rom(256, 0x0400);
    let rom = make_ines_mapper(&prg_rom, &chr_rom, 37);
    let mut cartridge = Cartridge::from_ines(&rom).expect("mapper 37 should parse");

    assert!(cartridge.cpu_write(0x8000, 0x06));
    assert!(cartridge.cpu_write(0x8001, 0x05));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x05));

    assert!(cartridge.cpu_write(0x6000, 0x03));
    // exReg 3: 0x08 (exReg & 3 == 3) | (5 & 7) = 13; exReg<<2 & 0x10 is 0 here.
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x0D));
    // Fixed slot raw $3E: 0x08 | (0x3E & 7)
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x0E));

    assert!(cartridge.cpu_write(0x8000, 0x00));
    assert!(cartridge.cpu_write(0x8001, 0x03));
    // CHR slot 0 uses effective value 2; (exReg<<5 & 0x80) is always 0 for
    // the 3-bit register, so the offset only comes from the MMC3 value.
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x02));
    assert_eq!(cartridge.ppu_read(0x0400), Some(0x03));
}

#[test]
fn mapper47_selects_128k_half_via_multicart_register() {
    let prg_rom = tagged_prg_rom(32, 0x2000);
    let chr_rom = tagged_chr_rom(256, 0x0400);
    let rom = make_ines_mapper(&prg_rom, &chr_rom, 47);
    let mut cartridge = Cartridge::from_ines(&rom).expect("mapper 47 should parse");

    assert!(cartridge.cpu_write(0x8000, 0x06));
    assert!(cartridge.cpu_write(0x8001, 0x05));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x05));

    assert!(cartridge.cpu_write(0x6000, 0x01));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x15));

    assert!(cartridge.cpu_write(0x6000, 0x00));
    assert!(cartridge.cpu_write(0x8000, 0x00));
    assert!(cartridge.cpu_write(0x8001, 0x03));
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x02));
    assert!(cartridge.cpu_write(0x6000, 0x01));
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x82));
}

#[test]
fn mapper49_switches_between_32k_and_mmc3_modes() {
    let prg_rom = tagged_prg_rom(64, 0x2000);
    let chr_rom = tagged_chr_rom(128, 0x0400);
    let rom = make_ines_mapper(&prg_rom, &chr_rom, 49);
    let mut cartridge = Cartridge::from_ines(&rom).expect("mapper 49 should parse");

    // Without $A001 bit 7 the multicart register stays inert.
    assert!(cartridge.cpu_write(0xA001, 0x00));
    assert!(cartridge.cpu_write(0x6000, 0x14));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x00));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x02));

    assert!(cartridge.cpu_write(0xA001, 0x80));
    assert!(cartridge.cpu_write(0x6000, 0x01));
    // MMC3 8K mode with game base 0: bank register 6 default 0, fixed $3F masked to 0x0F.
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x00));
    assert_eq!(cartridge.cpu_read(0xE000), Some(0x0F));

    assert!(cartridge.cpu_write(0x8000, 0x06));
    assert!(cartridge.cpu_write(0x8001, 0x0B));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x0B));

    // 32K direct mode through bit 4 of the multicart register (bank 2 of 16).
    assert!(cartridge.cpu_write(0x6000, 0x20));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x08));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x0A));

    // CHR: reg0 = 3 makes slot 0 use effective value 2; exReg 1 adds bit 8.
    assert!(cartridge.cpu_write(0x8000, 0x00));
    assert!(cartridge.cpu_write(0x8001, 0x03));
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x02));
}

#[test]
fn mapper206_banks_prg_and_fixed_chr_layout() {
    let prg_rom = tagged_prg_rom(16, 0x2000);
    let chr_rom = tagged_chr_rom(64, 0x0400);
    let rom = make_ines_mapper(&prg_rom, &chr_rom, 206);
    let mut cartridge = Cartridge::from_ines(&rom).expect("mapper 206 should parse");

    assert_eq!(cartridge.cpu_read(0x8000), Some(0x00));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x0E));
    assert_eq!(cartridge.cpu_read(0xE000), Some(0x0F));

    assert!(cartridge.cpu_write(0x8000, 0x06));
    assert!(cartridge.cpu_write(0x8001, 0x22));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x02));

    // CHR register 0 is a 4K value shifted into 2K banks.
    assert!(cartridge.cpu_write(0x8000, 0x00));
    assert!(cartridge.cpu_write(0x8001, 0x3F));
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x3E));
    assert_eq!(cartridge.ppu_read(0x0800), Some(0x00));
    assert!(cartridge.cpu_write(0x8000, 0x02));
    assert!(cartridge.cpu_write(0x8001, 0x05));
    assert_eq!(cartridge.ppu_read(0x1000), Some(0x05));
}

#[test]
fn mapper105_selects_prg_chip_and_runs_countdown_irq() {
    let prg_rom = tagged_prg_rom(16, 0x4000);
    let rom = make_ines_mapper(&prg_rom, &[], 105);
    let mut cartridge = Cartridge::from_ines(&rom).expect("mapper 105 should parse");

    // Default: 32K mode over the lower chip, $8000-$FFFF = banks 0-1.
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x00));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x01));

    // CHR bank 0 bit 3 switches to the upper chip (MMC1 mode 0xC).
    write_mmc1_register(&mut cartridge, 0xA000, 0x08);
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x08));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x0F));

    write_mmc1_register(&mut cartridge, 0xE000, 0x05);
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x0D));

    // Clearing bit 3 returns to 32K banking from CHR bank 0 (bank 1).
    write_mmc1_register(&mut cartridge, 0xA000, 0x02);
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x02));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x03));

    // Arming (bit 4 clear) starts the countdown; one cycle short is quiet.
    write_mmc1_register(&mut cartridge, 0xA000, 0x08);
    for _ in 0..(16 * 20 - 2) {
        cartridge.tick_cpu_cycle();
    }
    assert!(!cartridge.irq_line());
    cartridge.tick_cpu_cycle();
    assert!(cartridge.irq_line());

    // Committing bit 4 stops the timer and releases the IRQ.
    write_mmc1_register(&mut cartridge, 0xA000, 0x18);
    assert!(!cartridge.irq_line());

    // WRAM is live at $6000 and CHR RAM accepts writes.
    assert!(cartridge.cpu_write(0x6000, 0x42));
    assert_eq!(cartridge.cpu_read(0x6000), Some(0x42));
    assert!(cartridge.ppu_write(0x0010, 0x99));
    assert_eq!(cartridge.ppu_read(0x0010), Some(0x99));
}

#[test]
fn mapper200_banks_prg_chr_and_mirroring_from_address() {
    let prg_rom = tagged_prg_rom(8, 0x4000);
    let chr_rom = tagged_chr_rom(8, 0x2000);
    let rom = make_ines_mapper(&prg_rom, &chr_rom, 200);
    let mut cartridge = Cartridge::from_ines(&rom).expect("mapper 200 should parse");

    assert!(cartridge.cpu_write(0x8013, 0x00));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x03));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x03));
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x03));
    assert_eq!(cartridge.mirroring(), Mirroring::Vertical);

    assert!(cartridge.cpu_write(0x800B, 0x00));
    assert_eq!(cartridge.mirroring(), Mirroring::Horizontal);
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x03));
}

#[test]
fn mapper201_shares_bank_number_between_prg_and_chr() {
    let prg_rom = tagged_prg_rom(4, 0x8000);
    let chr_rom = tagged_chr_rom(4, 0x2000);
    let rom = make_ines_mapper(&prg_rom, &chr_rom, 201);
    let mut cartridge = Cartridge::from_ines(&rom).expect("mapper 201 should parse");

    assert!(cartridge.cpu_write(0x8002, 0x00));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x02));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x02));
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x02));
    assert_eq!(cartridge.ppu_read(0x1FFF), Some(0x02));
}

#[test]
fn mapper225_switches_prg_width_banks_chr_and_mirroring() {
    let prg_rom = tagged_prg_rom(64, 0x4000);
    let chr_rom = tagged_chr_rom(64, 0x2000);
    let rom = make_ines_mapper(&prg_rom, &chr_rom, 225);
    let mut cartridge = Cartridge::from_ines(&rom).expect("mapper 225 should parse");

    // 16K mode: bit 12 set, bank (addr>>7 & 0x1F) = 2 doubled with bit 6 -> 5.
    assert!(cartridge.cpu_write(0x9140, 0x00));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x05));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x05));
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x00));
    assert_eq!(cartridge.mirroring(), Mirroring::Vertical);

    // 32K mode with bit 13 requesting horizontal mirroring; bank (addr>>7 & 0x1F) = 1.
    assert!(cartridge.cpu_write(0xA080, 0x00));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x02));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x03));
    assert_eq!(cartridge.mirroring(), Mirroring::Horizontal);
}

#[test]
fn mapper226_combines_two_data_latches() {
    let prg_rom = tagged_prg_rom(64, 0x4000);
    let chr_rom = tagged_chr_rom(4, 0x2000);
    let rom = make_ines_mapper(&prg_rom, &chr_rom, 226);
    let mut cartridge = Cartridge::from_ines(&rom).expect("mapper 226 should parse");

    assert!(cartridge.cpu_write(0x8000, 0x21));
    assert!(cartridge.cpu_write(0x8001, 0x01));
    // 16K mode (reg0 bit 5): only (0x01<<5 & 0x20) contributes 0x20, doubled
    // plus bit 0 -> 0x41, which wraps to bank 1.
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x01));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x01));
    assert_eq!(cartridge.mirroring(), Mirroring::Horizontal);

    assert!(cartridge.cpu_write(0x8000, 0x40));
    // 32K mode: bank 0x20 wraps to 0, vertical mirroring.
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x00));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x01));
    assert_eq!(cartridge.mirroring(), Mirroring::Vertical);

    // CHR is a fixed window over the first bank.
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x00));
    assert_eq!(cartridge.ppu_read(0x1FFF), Some(0x00));
}

#[test]
fn mapper229_defaults_banks_zero_one_when_low_bits_clear() {
    let prg_rom = tagged_prg_rom(32, 0x4000);
    let chr_rom = tagged_chr_rom(32, 0x2000);
    let rom = make_ines_mapper(&prg_rom, &chr_rom, 229);
    let mut cartridge = Cartridge::from_ines(&rom).expect("mapper 229 should parse");

    assert!(cartridge.cpu_write(0x8000, 0x00));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x00));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x01));

    assert!(cartridge.cpu_write(0x8005, 0x00));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x05));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x05));
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x05));
    assert_eq!(cartridge.mirroring(), Mirroring::Vertical);

    assert!(cartridge.cpu_write(0x8025, 0x00));
    assert_eq!(cartridge.mirroring(), Mirroring::Horizontal);
}

#[test]
fn mapper240_latches_data_below_6000() {
    let prg_rom = tagged_prg_rom(16, 0x8000);
    let chr_rom = tagged_chr_rom(16, 0x2000);
    let rom = make_ines_mapper(&prg_rom, &chr_rom, 240);
    let mut cartridge = Cartridge::from_ines(&rom).expect("mapper 240 should parse");

    assert!(cartridge.cpu_write(0x4020, 0x21));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x02));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x02));
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x01));

    assert!(cartridge.cpu_write(0x6000, 0x77));
    assert_eq!(cartridge.cpu_read(0x6000), Some(0x77));
}

#[test]
fn mapper241_banks_32k_prg_from_written_data() {
    let prg_rom = tagged_prg_rom(32, 0x4000);
    let chr_rom = tagged_chr_rom(4, 0x2000);
    let rom = make_ines_mapper(&prg_rom, &chr_rom, 241);
    let mut cartridge = Cartridge::from_ines(&rom).expect("mapper 241 should parse");

    assert!(cartridge.cpu_write(0x8000, 0x03));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x06));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x07));

    assert!(cartridge.cpu_write(0x6000, 0x5A));
    assert_eq!(cartridge.cpu_read(0x6000), Some(0x5A));

    // Fixed CHR window over the first bank.
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x00));
}

#[test]
fn mapper242_banks_prg_from_address_bits() {
    let prg_rom = tagged_prg_rom(32, 0x4000);
    let chr_rom = tagged_chr_rom(4, 0x2000);
    let rom = make_ines_mapper(&prg_rom, &chr_rom, 242);
    let mut cartridge = Cartridge::from_ines(&rom).expect("mapper 242 should parse");

    assert!(cartridge.cpu_write(0x8018, 0x00));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x06));
    assert_eq!(cartridge.cpu_read(0xC000), Some(0x07));
    assert_eq!(cartridge.mirroring(), Mirroring::Vertical);

    assert!(cartridge.cpu_write(0x801A, 0x00));
    assert_eq!(cartridge.mirroring(), Mirroring::Horizontal);

    assert!(cartridge.cpu_write(0x6000, 0x33));
    assert_eq!(cartridge.cpu_read(0x6000), Some(0x33));
}

#[test]
fn mapper244_banks_through_specific_address_ranges() {
    let prg_rom = tagged_prg_rom(8, 0x4000);
    let chr_rom = tagged_chr_rom(8, 0x2000);
    let rom = make_ines_mapper(&prg_rom, &chr_rom, 244);
    let mut cartridge = Cartridge::from_ines(&rom).expect("mapper 244 should parse");

    assert_eq!(cartridge.cpu_read(0x8000), Some(0x00));

    assert!(cartridge.cpu_write(0x8067, 0x00));
    assert_eq!(cartridge.cpu_read(0x8000), Some(0x04));

    assert!(cartridge.cpu_write(0x80A7, 0x00));
    assert_eq!(cartridge.ppu_read(0x0000), Some(0x02));
}
