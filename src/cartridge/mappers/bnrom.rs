use super::Mapper;
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_4K: usize = 0x1000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Bnrom {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    prg_bank: usize,
    chr_lo_bank: usize,
    chr_hi_bank: usize,
    mirroring: Mirroring,
    is_nina: bool,
}

impl Bnrom {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let is_nina = !chr_rom.is_empty();
        let chr = if is_nina {
            ChrMemory::Rom(chr_rom)
        } else {
            ChrMemory::Ram(vec![0; CHR_BANK_LEN])
        };

        Self {
            prg_rom,
            chr,
            prg_bank: 0,
            chr_lo_bank: 0,
            chr_hi_bank: 0,
            mirroring,
            is_nina,
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_32K
    }

    fn chr_bank_count_4k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(chr_rom) => chr_rom.len() / CHR_BANK_4K,
            ChrMemory::Ram(chr_ram) => chr_ram.len() / CHR_BANK_4K,
        }
    }
}

impl Mapper for Bnrom {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xFFFF => {
                let bank = self.prg_bank % self.prg_bank_count().max(1);
                let offset = bank * PRG_BANK_32K + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        if self.is_nina {
            match addr {
                0x7FFD => {
                    self.prg_bank = data as usize;
                    true
                }
                0x7FFE => {
                    self.chr_lo_bank = data as usize;
                    true
                }
                0x7FFF => {
                    self.chr_hi_bank = data as usize;
                    true
                }
                _ => false,
            }
        } else {
            match addr {
                0x8000..=0xFFFF => {
                    self.prg_bank = (data & 0x0F) as usize;
                    true
                }
                _ => false,
            }
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let bank_count = self.chr_bank_count_4k().max(1);
        let chr_lo = self.chr_lo_bank;
        let chr_hi = self.chr_hi_bank;
        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => {
                let is_hi = addr >= 0x1000;
                let bank = if is_hi {
                    chr_hi % bank_count
                } else {
                    chr_lo % bank_count
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
        writer.write_u64(self.prg_bank as u64);
        writer.write_bool(self.is_nina);
        if self.is_nina {
            writer.write_u64(self.chr_lo_bank as u64);
            writer.write_u64(self.chr_hi_bank as u64);
        }
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.prg_bank = reader.read_u64()? as usize;
        self.is_nina = reader.read_bool()?;
        if self.is_nina {
            self.chr_lo_bank = reader.read_u64()? as usize;
            self.chr_hi_bank = reader.read_u64()? as usize;
        }
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for BNROM save state",
                ));
            }
        }
        Ok(())
    }
}
