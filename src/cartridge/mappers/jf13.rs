use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_8K: usize = 0x2000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Jf13 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    prg_bank: usize,
    chr_bank: usize,
    mirroring: Mirroring,
}

impl Jf13 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        }
    }
}

impl Mapper for Jf13 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let offset = self.prg_bank * PRG_BANK_32K + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x6000..=0x6FFF => {
                self.prg_bank = ((data >> 4) & 0x03) as usize;
                self.chr_bank = ((data & 0x03) | ((data & 0x40) >> 4)) as usize;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let offset = self.chr_bank * CHR_BANK_8K + addr as usize;
        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset % chr_ram.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let offset = self.chr_bank * CHR_BANK_8K + addr as usize;
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            let len = chr_ram.len();
            chr_ram[offset % len] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.prg_bank as u8);
        writer.write_u8(self.chr_bank as u8);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.prg_bank = reader.read_u8()? as usize;
        self.chr_bank = reader.read_u8()? as usize;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}

fn encode_mirroring(mirroring: Mirroring) -> u8 {
    match mirroring {
        Mirroring::Horizontal => 0,
        Mirroring::Vertical => 1,
        Mirroring::FourScreen => 2,
        Mirroring::SPAGE0 => 3,
        Mirroring::SPAGE1 => 4,
    }
}

fn decode_mirroring(encoded: u8) -> Result<Mirroring, SaveStateError> {
    match encoded {
        0 => Ok(Mirroring::Horizontal),
        1 => Ok(Mirroring::Vertical),
        2 => Ok(Mirroring::FourScreen),
        3 => Ok(Mirroring::SPAGE0),
        4 => Ok(Mirroring::SPAGE1),
        _ => Err(SaveStateError::InvalidData(
            "invalid JF-13 mirroring value",
        )),
    }
}
