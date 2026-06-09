use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_8K: usize = 0x2000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Mapper46 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    reg0: u8,
    reg1: u8,
    mirroring: Mirroring,
}

impl Mapper46 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            reg0: 0,
            reg1: 0,
            mirroring,
        }
    }
}

impl Mapper for Mapper46 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let bank = (self.reg1 as usize & 1) + ((self.reg0 as usize & 0x0F) << 1);
                let offset = bank * PRG_BANK_32K + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x6000..=0x7FFF => {
                self.reg0 = data;
                true
            }
            0x8000..=0xFFFF => {
                self.reg1 = data;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let bank = ((self.reg1 as usize >> 4) & 7) + ((self.reg0 as usize & 0xF0) >> 1);
        let offset = bank * CHR_BANK_8K + addr as usize;
        match &mut self.chr {
            ChrMemory::Rom(r) => Some(r[offset % r.len()]),
            ChrMemory::Ram(r) => Some(r[offset % r.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let bank = ((self.reg1 as usize >> 4) & 7) + ((self.reg0 as usize & 0xF0) >> 1);
        let offset = bank * CHR_BANK_8K + addr as usize;
        if let ChrMemory::Ram(r) = &mut self.chr {
            let len = r.len();
            r[offset % len] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.reg0);
        writer.write_u8(self.reg1);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.reg0 = reader.read_u8()?;
        self.reg1 = reader.read_u8()?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}
