use super::*;
use crate::input::{ControllerButton, ControllerState};

fn write_mmc1_register(bus: &mut NESBus, addr: u16, value: u8) {
    for bit in 0..5 {
        bus.cpu_write(addr, (value >> bit) & 0x01);
    }
}

fn make_ines(prg_banks: u8, chr_banks: u8, flags6: u8) -> Vec<u8> {
    let mut rom = vec![0; 16];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = prg_banks;
    rom[5] = chr_banks;
    rom[6] = flags6;

    let prg_len = prg_banks as usize * 0x4000;
    let chr_len = chr_banks as usize * 0x2000;
    rom.extend((0..prg_len).map(|index| (index & 0xFF) as u8));
    rom.extend((0..chr_len).map(|index| (0x80 | (index & 0x7F)) as u8));
    rom
}

fn make_ines_mapper(prg_banks: u8, chr_banks: u8, mapper_id: u16, flags6_low: u8) -> Vec<u8> {
    let mut rom = make_ines(
        prg_banks,
        chr_banks,
        ((mapper_id as u8) << 4) | (flags6_low & 0x0F),
    );
    rom[7] = (mapper_id & 0xF0) as u8;
    rom
}

#[test]
fn cartridge_prg_rom_is_visible_on_cpu_bus() {
    let mut bus = NESBus::new();
    let rom = make_ines(1, 1, 0x00);

    bus.load_cartridge_ines(&rom).expect("NROM should load");

    assert_eq!(bus.cpu_read(0x8000), 0x00);
    assert_eq!(bus.cpu_read(0x8001), 0x01);
    assert_eq!(bus.cpu_read(0xC000), 0x00);
    assert_eq!(bus.cpu_read(0xFFFF), 0xFF);
}

#[test]
fn cartridge_chr_rom_is_visible_through_ppu_registers() {
    let mut bus = NESBus::new();
    let rom = make_ines(1, 1, 0x00);

    bus.load_cartridge_ines(&rom).expect("NROM should load");
    bus.cpu_write(0x2006, 0x00);
    bus.cpu_write(0x2006, 0x00);

    assert_eq!(
        bus.cpu_read(0x2007),
        0x00,
        "first read should return the old buffer"
    );
    assert_eq!(
        bus.cpu_read(0x2007),
        0x80,
        "second read should return CHR byte 0"
    );
    assert_eq!(
        bus.cpu_read(0x2007),
        0x81,
        "buffer should advance through CHR"
    );
}

#[test]
fn nametable_access_uses_cartridge_mirroring() {
    let mut bus = NESBus::new();
    let rom = make_ines(1, 0, 0x01);

    bus.load_cartridge_ines(&rom)
        .expect("vertical mirroring NROM should load");

    bus.cpu_write(0x2006, 0x20);
    bus.cpu_write(0x2006, 0x00);
    bus.cpu_write(0x2007, 0x3C);

    bus.cpu_write(0x2006, 0x28);
    bus.cpu_write(0x2006, 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0x3C);
}

#[test]
fn nametable_access_tracks_mmc1_runtime_mirroring_changes() {
    let mut bus = NESBus::new();
    let rom = make_ines(2, 0, 0x10);

    bus.load_cartridge_ines(&rom).expect("MMC1 should load");

    bus.cpu_write(0x2006, 0x20);
    bus.cpu_write(0x2006, 0x00);
    bus.cpu_write(0x2007, 0x5A);

    bus.cpu_write(0x2006, 0x24);
    bus.cpu_write(0x2006, 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0x5A);

    write_mmc1_register(&mut bus, 0x8000, 0x02);

    bus.cpu_write(0x2006, 0x20);
    bus.cpu_write(0x2006, 0x00);
    bus.cpu_write(0x2007, 0xA5);

    bus.cpu_write(0x2006, 0x28);
    bus.cpu_write(0x2006, 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0xA5);
}

#[test]
fn nametable_access_tracks_mapper118_chr_bank_mirroring_in_2k_mode() {
    let mut bus = NESBus::new();
    let rom = make_ines_mapper(2, 1, 118, 0x00);

    bus.load_cartridge_ines(&rom)
        .expect("Mapper 118 should load");

    bus.cpu_write(0x8000, 0x00);
    bus.cpu_write(0x8001, 0x00);
    bus.cpu_write(0x8000, 0x01);
    bus.cpu_write(0x8001, 0x80);

    bus.cpu_write(0x2006, 0x20);
    bus.cpu_write(0x2006, 0x00);
    bus.cpu_write(0x2007, 0x11);

    bus.cpu_write(0x2006, 0x28);
    bus.cpu_write(0x2006, 0x00);
    bus.cpu_write(0x2007, 0x22);

    bus.cpu_write(0x2006, 0x24);
    bus.cpu_write(0x2006, 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0x11);

    bus.cpu_write(0x2006, 0x2C);
    bus.cpu_write(0x2006, 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0x22);

    bus.cpu_write(0xA000, 0x00);
    bus.cpu_write(0x2006, 0x24);
    bus.cpu_write(0x2006, 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0x11);
}

#[test]
fn nametable_access_tracks_mapper118_chr_bank_mirroring_in_1k_mode() {
    let mut bus = NESBus::new();
    let rom = make_ines_mapper(2, 1, 118, 0x00);

    bus.load_cartridge_ines(&rom)
        .expect("Mapper 118 should load");

    bus.cpu_write(0x8000, 0x82);
    bus.cpu_write(0x8001, 0x80);
    bus.cpu_write(0x8000, 0x83);
    bus.cpu_write(0x8001, 0x00);
    bus.cpu_write(0x8000, 0x84);
    bus.cpu_write(0x8001, 0x80);
    bus.cpu_write(0x8000, 0x85);
    bus.cpu_write(0x8001, 0x00);

    bus.cpu_write(0x2006, 0x20);
    bus.cpu_write(0x2006, 0x00);
    bus.cpu_write(0x2007, 0xA1);

    bus.cpu_write(0x2006, 0x24);
    bus.cpu_write(0x2006, 0x00);
    bus.cpu_write(0x2007, 0xB2);

    bus.cpu_write(0x2006, 0x28);
    bus.cpu_write(0x2006, 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0xA1);

    bus.cpu_write(0x2006, 0x2C);
    bus.cpu_write(0x2006, 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0x00);
    assert_eq!(bus.cpu_read(0x2007), 0xB2);
}

#[test]
fn controller_reads_shift_latched_buttons_in_standard_order() {
    let mut bus = NESBus::new();
    let mut state = ControllerState::new();
    state.set_pressed(ControllerButton::A, true);
    state.set_pressed(ControllerButton::Select, true);
    state.set_pressed(ControllerButton::Left, true);
    bus.set_controller_state(0, state);

    bus.cpu_write(0x4016, 0x01);
    bus.cpu_write(0x4016, 0x00);

    let reads: Vec<u8> = (0..8).map(|_| bus.cpu_read(0x4016)).collect();

    assert_eq!(reads, vec![1, 0, 1, 0, 0, 0, 1, 0]);
    assert_eq!(bus.cpu_read(0x4016), 1);
    assert_eq!(bus.cpu_read(0x4016), 1);
}

#[test]
fn controller_strobe_high_keeps_reporting_live_a_button_without_advancing() {
    let mut bus = NESBus::new();
    bus.set_controller_state(0, ControllerState::from_bits(0x01));
    bus.cpu_write(0x4016, 0x01);

    assert_eq!(bus.cpu_read(0x4016), 1);
    assert_eq!(bus.cpu_read(0x4016), 1);

    bus.set_controller_state(0, ControllerState::from_bits(0x00));

    assert_eq!(bus.cpu_read(0x4016), 0);
}

#[test]
fn second_controller_reads_from_4017() {
    let mut bus = NESBus::new();
    let mut state = ControllerState::new();
    state.set_pressed(ControllerButton::B, true);
    bus.set_controller_state(1, state);

    bus.cpu_write(0x4016, 0x01);
    bus.cpu_write(0x4016, 0x00);

    assert_eq!(bus.cpu_read(0x4017), 0);
    assert_eq!(bus.cpu_read(0x4017), 1);
}

#[test]
fn unmapped_cpu_reads_return_the_open_bus_value() {
    let mut bus = NESBus::new();

    bus.cpu_write(0x0000, 0x5A);

    assert_eq!(bus.cpu_read(0x4018), 0x5A);
}

#[test]
fn controller_reads_preserve_open_bus_in_upper_bits() {
    let mut bus = NESBus::new();
    bus.set_controller_state(0, ControllerState::from_bits(0x01));
    bus.cpu_write(0x4016, 0x01);
    bus.cpu_write(0x4016, 0x00);
    bus.cpu_write(0x0000, 0xE0);

    assert_eq!(bus.cpu_read(0x4016), 0xE1);
}

#[test]
fn apu_frame_irq_flag_is_visible_in_4015_and_clears_on_read() {
    let mut bus = NESBus::new();
    bus.cpu_write(0x4017, 0x00);

    for _ in 0..30_000 {
        bus.tick_apu_cpu_cycle();
    }

    assert_eq!(bus.cpu_read(0x4015) & 0x40, 0x40);
    for _ in 0..8 {
        bus.tick_apu_cpu_cycle();
    }
    assert_eq!(bus.cpu_read(0x4015) & 0x40, 0x00);
}

#[test]
fn apu_frame_irq_inhibit_write_clears_pending_flag() {
    let mut bus = NESBus::new();
    bus.cpu_write(0x4017, 0x00);

    for _ in 0..29_832 {
        bus.tick_apu_cpu_cycle();
    }

    bus.cpu_write(0x4017, 0x40);

    assert_eq!(bus.cpu_read(0x4015) & 0x40, 0x00);
}
