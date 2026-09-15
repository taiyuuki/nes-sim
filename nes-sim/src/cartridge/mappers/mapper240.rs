use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_8K: usize = 0x2000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// CNE Shui Hu Zhuan board (iNES mapper 240). A data latch at $4020-$5FFF
/// selects the 32 KiB PRG and 8 KiB CHR banks; $6000-$7FFF is WRAM.
pub(super) struct Mapper240 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    prg_bank: usize,
    chr_bank: usize,
    mirroring: Mirroring,
}

impl Mapper240 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; crate::cartridge::CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            prg_ram: vec![0; PRG_RAM_LEN],
            chr,
            prg_bank: 0,
            chr_bank: 0,
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

impl Mapper for Mapper240 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0xFFFF => {
                let bank = self.prg_bank % self.prg_bank_count();
                Some(self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x4020..=0x5FFF => {
                self.prg_bank = (data as usize) >> 4;
                self.chr_bank = data as usize;
                true
            }
            0x6000..=0x7FFF => {
                self.prg_ram[(addr - 0x6000) as usize] = data;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let offset = (self.chr_bank % self.chr_bank_count()) * CHR_BANK_8K + addr as usize;
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let offset = (self.chr_bank % self.chr_bank_count()) * CHR_BANK_8K + addr as usize;
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            chr_ram[offset] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.prg_ram);
        writer.write_u8(self.prg_bank as u8);
        writer.write_u8(self.chr_bank as u8);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_ram)?;
        self.prg_bank = reader.read_u8()? as usize;
        self.chr_bank = reader.read_u8()? as usize;
        Ok(())
    }
}
