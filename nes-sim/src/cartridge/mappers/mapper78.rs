use super::Mapper;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::cartridge::{CHR_BANK_LEN, Mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Mapper78 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    prg_bank: usize,
    chr_bank: usize,
    mirroring: Mirroring,
}

impl Mapper78 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_LEN])
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

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_16K
    }

    fn chr_bank_count(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(chr_rom) => chr_rom.len() / CHR_BANK_LEN,
            ChrMemory::Ram(chr_ram) => chr_ram.len() / CHR_BANK_LEN,
        }
    }
}

impl Mapper for Mapper78 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.prg_bank % self.prg_bank_count().max(1);
                let offset = bank * PRG_BANK_16K + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xC000..=0xFFFF => {
                let last = self.prg_bank_count().saturating_sub(1);
                let offset = last * PRG_BANK_16K + (addr as usize - 0xC000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0xFFFF => {
                self.prg_bank = (data & 0x07) as usize;
                self.chr_bank = (data >> 4) as usize;

                let bit3 = (data >> 3) & 1;
                if bit3 != 0 {
                    self.mirroring = Mirroring::Vertical;
                } else {
                    self.mirroring = Mirroring::Horizontal;
                }

                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let bank = self.chr_bank % self.chr_bank_count().max(1);
        let offset = bank * CHR_BANK_LEN + addr as usize;
        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset % chr_ram.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let bank = self.chr_bank % self.chr_bank_count().max(1);
        let offset = bank * CHR_BANK_LEN + addr as usize;
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
        writer.write_u64(self.prg_bank as u64);
        writer.write_u64(self.chr_bank as u64);
        writer.write_u8(encode_mirroring(self.mirroring));
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
        self.chr_bank = reader.read_u64()? as usize;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for Mapper78 save state",
                ));
            }
        }
        Ok(())
    }
}
