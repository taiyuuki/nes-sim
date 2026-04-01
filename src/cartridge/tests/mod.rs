use super::*;

const CHR_HALF_BANK_LEN: usize = 0x1000;

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
    let rom = make_ines(1, 1, 0x40, 0x00, 0x00);

    let err = match Cartridge::from_ines(&rom) {
        Ok(_) => panic!("mapper 4 should be rejected"),
        Err(err) => err,
    };

    assert_eq!(err, CartridgeError::UnsupportedMapper(4));
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
