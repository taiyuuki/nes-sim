use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_8K: usize = 0x2000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// BMC 21-in-1 (iNES mapper 201). The write address latches one bank number
/// shared by the 32 KiB PRG and 8 KiB CHR windows.
pub(super) struct Mapper201 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    bank: usize,
    mirroring: Mirroring,
}

impl Mapper201 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; crate::cartridge::CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            bank: 0,
            mirroring,
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_32K).max(1)
    }

    fn chr_bank_count(&self) -> usize {
        let len = match &self.chr {
            ChrMemory::Rom(chr_rom) => chr_rom.len(),
            ChrMemory::Ram(chr_ram) => chr_ram.len(),
        };
        (len / CHR_BANK_8K).max(1)
    }
}

impl Mapper for Mapper201 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let bank = self.bank % self.prg_bank_count();
                Some(self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, _data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.bank = addr as usize;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let offset = (self.bank % self.chr_bank_count()) * CHR_BANK_8K + addr as usize;
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let offset = (self.bank % self.chr_bank_count()) * CHR_BANK_8K + addr as usize;
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            chr_ram[offset] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u16(self.bank as u16);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.bank = reader.read_u16()? as usize;
        Ok(())
    }
}
