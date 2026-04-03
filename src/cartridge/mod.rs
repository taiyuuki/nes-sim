use std::error::Error;
use std::fmt::{Display, Formatter};

mod mappers;

use self::mappers::{Mapper, from_mapper_id};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const INES_HEADER_LEN: usize = 16;
const TRAINER_LEN: usize = 512;
const PRG_BANK_LEN: usize = 0x4000;
const CHR_BANK_LEN: usize = 0x2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mirroring {
    Horizontal,
    Vertical,
    FourScreen,
    SPAGE0,
    SPAGE1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TVSystem {
    NTSC,
    PAL,
    DENDY,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CartridgeError {
    FileTooSmall,
    InvalidMagic,
    Nes2Unsupported,
    UnsupportedMapper(u16),
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

pub struct Cartridge {
    mapper: Box<dyn Mapper>,
    has_sram: bool,
    has_trainer: bool,
    tv_system: TVSystem,
    is_ines2: bool,
    submapper: u8,
}

impl Cartridge {
    pub fn from_ines(rom: &[u8]) -> Result<Self, CartridgeError> {
        Self::from_ines_with_tv_system_override(rom, None)
    }

    pub fn from_ines_with_tv_system_override(
        rom: &[u8],
        tv_system_override: Option<TVSystem>,
    ) -> Result<Self, CartridgeError> {
        if rom.len() < INES_HEADER_LEN {
            return Err(CartridgeError::FileTooSmall);
        }

        if &rom[0..4] != b"NES\x1A" {
            return Err(CartridgeError::InvalidMagic);
        }

        let flags6 = rom[6];
        let flags7 = rom[7];
        let flags8 = rom[8];

        let mut mapper_id = u16::from(flags6 >> 4) | u16::from(flags7 & 0xF0);
        let mirroring = if (flags6 & 0x08) != 0 {
            Mirroring::FourScreen
        } else if (flags6 & 0x01) != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };

        let has_sram = (flags6 & 0x02) != 0;
        let has_trainer = (flags6 & 0x04) != 0;
        let trainer_len = if has_trainer { TRAINER_LEN } else { 0 };

        let prg_len = rom[4] as usize * PRG_BANK_LEN;
        let chr_len = rom[5] as usize * CHR_BANK_LEN;
        let data_start = INES_HEADER_LEN + trainer_len;
        let data_end = data_start + prg_len + chr_len;
        if rom.len() < data_end {
            return Err(CartridgeError::TruncatedData);
        }

        // NES 2.0
        let is_ines2 = (flags7 & 0x0C) == 0x08;
        let mut submapper = 0;
        if is_ines2 {
            mapper_id |= u16::from(flags8 & 0x0F) << 8;
            submapper = flags8 >> 4;
            // return Err(CartridgeError::Nes2Unsupported);
        }
        let tv_system = tv_system_override.unwrap_or_else(|| detect_tv_system(rom, is_ines2));
        let prg_rom = rom[data_start..data_start + prg_len].to_vec();
        let chr_rom = rom[data_start + prg_len..data_end].to_vec();
        let mapper = from_mapper_id(mapper_id, mirroring, prg_rom, chr_rom)?;

        Ok(Self {
            mapper,
            submapper,
            has_sram,
            has_trainer,
            tv_system,
            is_ines2,
        })
    }

    pub fn mirroring(&self) -> Mirroring {
        self.mapper.mirroring()
    }

    pub fn tv_system(&self) -> TVSystem {
        self.tv_system
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

    pub fn check_a12(&mut self, addr: u16, ppu_cycle: u64) {
        self.mapper.check_a12(addr, ppu_cycle);
    }

    pub fn map_nametable_addr(&self, addr: u16) -> Option<usize> {
        self.mapper.map_nametable_addr(addr)
    }

    pub fn irq_line(&self) -> bool {
        self.mapper.irq_line()
    }

    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u16(self.mapper.mapper_id());
        self.mapper.save_state(writer);
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        let actual = reader.read_u16()?;
        let expected = self.mapper.mapper_id();
        if actual != expected {
            return Err(SaveStateError::MapperMismatch { expected, actual });
        }
        self.mapper.load_state(reader)
    }
}

fn detect_tv_system(header: &[u8], is_ines2: bool) -> TVSystem {
    if is_ines2 {
        return match header[12] & 0x03 {
            0x01 => TVSystem::PAL,
            0x03 => TVSystem::DENDY,
            _ => TVSystem::NTSC,
        };
    }

    // iNES 1.0 byte 9 is notoriously unreliable unless the trailing extension bytes are clean.
    if header[11..16].iter().any(|&byte| byte != 0) {
        return TVSystem::NTSC;
    }

    if (header[9] & 0x01) != 0 || (header[10] & 0x03) == 0x02 {
        TVSystem::PAL
    } else {
        TVSystem::NTSC
    }
}

#[cfg(test)]
mod tests;
