use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_8K: usize = 0x2000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// Sunsoft-2 board (iNES mapper 089, Mito Koumon).
pub(super) struct Sunsoft2 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    bank: u8,
    mirroring: Mirroring,
}

impl Sunsoft2 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_8K])
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

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn chr_bank_count_8k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_8K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_8K).max(1),
        }
    }
}

impl Mapper for Sunsoft2 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let bank = ((self.bank >> 4) & 0x07) as usize % self.prg_bank_count_16k();
                Some(self.prg_rom[bank * PRG_BANK_16K + (addr as usize - 0x8000)])
            }
            0xC000..=0xFFFF => {
                let last = self.prg_bank_count_16k() - 1;
                Some(self.prg_rom[last * PRG_BANK_16K + (addr as usize - 0xC000)])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.bank = data;
                self.mirroring = if (data & 0x08) != 0 {
                    Mirroring::SPAGE1
                } else {
                    Mirroring::SPAGE0
                };
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let bank =
            ((self.bank & 0x07) | ((self.bank >> 7) << 3)) as usize % self.chr_bank_count_8k();
        let offset = bank * CHR_BANK_8K + addr as usize;
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset % chr_ram.len()]),
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
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
        writer.write_u8(self.bank);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for Sunsoft-2 save state",
                ));
            }
        }
        self.bank = reader.read_u8()?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}
