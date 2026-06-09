use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const CHR_BANK_4K: usize = 0x1000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct CpROM {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    chr_hi_bank: usize,
    mirroring: Mirroring,
}

impl CpROM {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_LEN])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            chr,
            chr_hi_bank: 0,
            mirroring,
        }
    }

    fn chr_bank_count_4k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(chr_rom) => chr_rom.len() / CHR_BANK_4K,
            ChrMemory::Ram(chr_ram) => chr_ram.len() / CHR_BANK_4K,
        }
    }
}

impl Mapper for CpROM {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let offset = (addr - 0x8000) as usize;
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.chr_hi_bank = (data & 0x03) as usize;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let bank_count = self.chr_bank_count_4k().max(1);
        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => {
                let bank = if addr < 0x1000 {
                    0
                } else {
                    self.chr_hi_bank % bank_count
                };
                let offset = bank * CHR_BANK_4K + (addr as usize & 0x0FFF);
                Some(chr_rom[offset % chr_rom.len()])
            }
            ChrMemory::Ram(chr_ram) => Some(chr_ram[addr as usize]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            chr_ram[addr as usize] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u64(self.chr_hi_bank as u64);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.chr_hi_bank = reader.read_u64()? as usize;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for CPROM save state",
                ));
            }
        }
        Ok(())
    }
}
