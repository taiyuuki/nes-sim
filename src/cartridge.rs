use std::error::Error;
use std::fmt::{Display, Formatter};

const INES_HEADER_LEN: usize = 16;
const TRAINER_LEN: usize = 512;
const PRG_BANK_LEN: usize = 0x4000;
const CHR_BANK_LEN: usize = 0x2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
    SS_MIRROR_0,
    SS_MIRROR_1,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CartridgeError {
    FileTooSmall,
    InvalidMagic,
    Nes2Unsupported,
    UnsupportedMapper(u8),
    TruncatedData,
}

impl Display for CartridgeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileTooSmall => f.write_str("ROM is smaller than the 16-byte iNES header"),
            Self::InvalidMagic => f.write_str("ROM is not in iNES format"),
            Self::Nes2Unsupported => f.write_str("NES 2.0 ROMs are not supported yet"),
            Self::UnsupportedMapper(id) => write!(f, "mapper {} is not supported yet", id),
            Self::TruncatedData => f.write_str("ROM ended before PRG/CHR data was fully present"),
        }
    }
}

impl Error for CartridgeError {}

trait Mapper {
    fn cpu_read(&mut self, addr: u16) -> Option<u8>;
    fn cpu_write(&mut self, addr: u16, data: u8) -> bool;
    fn ppu_read(&mut self, addr: u16) -> Option<u8>;
    fn ppu_write(&mut self, addr: u16, data: u8) -> bool;
}

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

struct Nrom {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
}

impl Nrom {
    fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            prg_ram: vec![0; 0x2000],
            chr,
        }
    }

    fn prg_rom_index(&self, addr: u16) -> usize {
        let offset = (addr - 0x8000) as usize;
        offset % self.prg_rom.len()
    }
}

impl Mapper for Nrom {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0xFFFF => Some(self.prg_rom[self.prg_rom_index(addr)]),
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x6000..=0x7FFF => {
                self.prg_ram[(addr - 0x6000) as usize] = data;
                true
            }
            0x8000..=0xFFFF => true,
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        match (&mut self.chr, addr) {
            (ChrMemory::Rom(chr_rom), 0x0000..=0x1FFF) => Some(chr_rom[addr as usize]),
            (ChrMemory::Ram(chr_ram), 0x0000..=0x1FFF) => Some(chr_ram[addr as usize]),
            _ => None,
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        match (&mut self.chr, addr) {
            (ChrMemory::Ram(chr_ram), 0x0000..=0x1FFF) => {
                chr_ram[addr as usize] = data;
                true
            }
            (ChrMemory::Rom(_), 0x0000..=0x1FFF) => true,
            _ => false,
        }
    }
}

pub struct Cartridge {
    mapper: Box<dyn Mapper>,
    mirroring: Mirroring,
}

impl Cartridge {
    pub fn from_ines(rom: &[u8]) -> Result<Self, CartridgeError> {
        if rom.len() < INES_HEADER_LEN {
            return Err(CartridgeError::FileTooSmall);
        }

        if &rom[0..4] != b"NES\x1A" {
            return Err(CartridgeError::InvalidMagic);
        }

        let flags6 = rom[6];
        let flags7 = rom[7];
        if (flags7 & 0x0C) == 0x08 {
            return Err(CartridgeError::Nes2Unsupported);
        }

        let mapper_id = (flags6 >> 4) | (flags7 & 0xF0);
        let mirroring = if (flags6 & 0x08) != 0 {
            Mirroring::FourScreen
        } else if (flags6 & 0x01) != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        let trainer_len = if (flags6 & 0x04) != 0 { TRAINER_LEN } else { 0 };
        let prg_len = rom[4] as usize * PRG_BANK_LEN;
        let chr_len = rom[5] as usize * CHR_BANK_LEN;
        let data_start = INES_HEADER_LEN + trainer_len;
        let data_end = data_start + prg_len + chr_len;
        if rom.len() < data_end {
            return Err(CartridgeError::TruncatedData);
        }

        let prg_rom = rom[data_start..data_start + prg_len].to_vec();
        let chr_rom = rom[data_start + prg_len..data_end].to_vec();

        let mapper: Box<dyn Mapper> = match mapper_id {
            0 => Box::new(Nrom::new(prg_rom, chr_rom)),
            _ => return Err(CartridgeError::UnsupportedMapper(mapper_id)),
        };

        Ok(Self { mapper, mirroring })
    }

    pub fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    pub fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        self.mapper.cpu_read(addr)
    }

    pub fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        self.mapper.cpu_write(addr, data)
    }

    pub fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        self.mapper.ppu_read(addr)
    }

    pub fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        self.mapper.ppu_write(addr, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let rom = make_ines(1, 1, 0x10, 0x00, 0x00);

        let err = match Cartridge::from_ines(&rom) {
            Ok(_) => panic!("mapper 1 should be rejected"),
            Err(err) => err,
        };

        assert_eq!(err, CartridgeError::UnsupportedMapper(1));
    }
}
