use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_4K: usize = 0x1000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// Konami VRC1 board (iNES mapper 075, Ganbare Goemon! / King Kong 2).
pub(super) struct Vrc1 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    prg_banks: [u8; 3],
    chr_banks: [u8; 2],
    mirroring: Mirroring,
}

impl Vrc1 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            chr,
            prg_banks: [0, 1, 2],
            chr_banks: [0, 0],
            mirroring,
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_8K
    }

    fn chr_bank_count_4k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_4K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_4K).max(1),
        }
    }
}

impl Mapper for Vrc1 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xDFFF => {
                let slot = ((addr - 0x8000) as usize) / PRG_BANK_8K;
                let bank = (self.prg_banks[slot] & 0x0F) as usize % self.prg_bank_count();
                Some(self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)])
            }
            0xE000..=0xFFFF => {
                let last = self.prg_rom.len() - PRG_BANK_8K;
                Some(self.prg_rom[last + (addr as usize - 0xE000)])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0x8FFF => {
                self.prg_banks[0] = data;
                true
            }
            0x9000..=0x9FFF => {
                self.mirroring = if (data & 0x01) != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
                self.chr_banks[0] = (self.chr_banks[0] & 0x0F) | ((data << 3) & 0x10);
                self.chr_banks[1] = (self.chr_banks[1] & 0x0F) | ((data << 2) & 0x10);
                true
            }
            0xA000..=0xAFFF => {
                self.prg_banks[1] = data;
                true
            }
            0xC000..=0xCFFF => {
                self.prg_banks[2] = data;
                true
            }
            0xE000..=0xEFFF => {
                self.chr_banks[0] = (self.chr_banks[0] & 0x10) | (data & 0x0F);
                true
            }
            0xF000..=0xFFFF => {
                self.chr_banks[1] = (self.chr_banks[1] & 0x10) | (data & 0x0F);
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let slot = addr as usize / CHR_BANK_4K;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_4k();
        let offset = bank * CHR_BANK_4K + (addr as usize & 0x0FFF);
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset % chr_ram.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let slot = addr as usize / CHR_BANK_4K;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_4k();
        let offset = bank * CHR_BANK_4K + (addr as usize & 0x0FFF);
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
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
        writer.write_bytes(&self.prg_banks);
        writer.write_bytes(&self.chr_banks);
        writer.write_u8(encode_mirroring(self.mirroring));
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for VRC1 save state",
                ));
            }
        }
        reader.read_bytes_into(&mut self.prg_banks)?;
        reader.read_bytes_into(&mut self.chr_banks)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        Ok(())
    }
}
