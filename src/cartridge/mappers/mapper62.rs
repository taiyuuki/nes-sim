use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_8K: usize = 0x2000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Mapper62 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    mode: u16,
    bank: u8,
}

impl Mapper62 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, _mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            mode: 0,
            bank: 0,
        }
    }
}

impl Mapper for Mapper62 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                if self.mode & 0x20 != 0 {
                    let bank16 = ((self.mode & 0x40) | ((self.mode >> 8) & 0x3F)) as usize;
                    let offset = bank16 * PRG_BANK_16K + (addr as usize & 0x3FFF);
                    Some(self.prg_rom[offset % self.prg_rom.len()])
                } else {
                    let bank32 = (((self.mode & 0x40) | ((self.mode >> 8) & 0x3F)) >> 1) as usize;
                    let offset = bank32 * PRG_BANK_32K + (addr as usize - 0x8000);
                    Some(self.prg_rom[offset % self.prg_rom.len()])
                }
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.mode = addr & 0x3FFF;
                self.bank = data & 0x03;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let chr_bank = ((self.mode as usize & 0x1F) << 2) | (self.bank as usize & 0x03);
        let offset = chr_bank * CHR_BANK_8K + addr as usize;
        match &mut self.chr {
            ChrMemory::Rom(r) => Some(r[offset % r.len()]),
            ChrMemory::Ram(r) => Some(r[offset % r.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let chr_bank = ((self.mode as usize & 0x1F) << 2) | (self.bank as usize & 0x03);
        let offset = chr_bank * CHR_BANK_8K + addr as usize;
        if let ChrMemory::Ram(r) = &mut self.chr {
            let len = r.len();
            r[offset % len] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        if ((self.mode >> 7) & 1) ^ 1 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        }
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u16(self.mode);
        writer.write_u8(self.bank);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.mode = reader.read_u16()?;
        self.bank = reader.read_u8()?;
        Ok(())
    }
}
