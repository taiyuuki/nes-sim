use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_2K: usize = 0x0800;
const CHR_BANK_1K: usize = 0x0400;
const WRAM_LEN: usize = 256;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct TaitoX1005 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    wram: Vec<u8>,
    wram_enable: u8,
    prg_banks: [u8; 3],
    chr_banks: [u8; 6],
    mirroring: Mirroring,
}

impl TaitoX1005 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            chr,
            wram: vec![0; WRAM_LEN],
            wram_enable: 0xFF,
            prg_banks: [0, 1, 2],
            chr_banks: [0; 6],
            mirroring,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_8K
    }

    fn chr_bank_count_2k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_2K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_2K).max(1),
        }
    }

    fn chr_bank_count_1k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_1K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_1K).max(1),
        }
    }
}

impl Mapper for TaitoX1005 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x7F00..=0x7FFF => {
                if self.wram_enable == 0xA3 {
                    Some(self.wram[(addr & 0xFF) as usize])
                } else {
                    Some(0xFF)
                }
            }
            0x8000..=0x9FFF => {
                let bank = self.prg_banks[0] as usize % self.prg_bank_count_8k();
                let offset = bank * PRG_BANK_8K + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xA000..=0xBFFF => {
                let bank = self.prg_banks[1] as usize % self.prg_bank_count_8k();
                let offset = bank * PRG_BANK_8K + (addr as usize - 0xA000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xC000..=0xDFFF => {
                let bank = self.prg_banks[2] as usize % self.prg_bank_count_8k();
                let offset = bank * PRG_BANK_8K + (addr as usize - 0xC000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xE000..=0xFFFF => {
                let last = self.prg_bank_count_8k().saturating_sub(1);
                let offset = last * PRG_BANK_8K + (addr as usize - 0xE000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x7F00..=0x7FFF => {
                if self.wram_enable == 0xA3 {
                    self.wram[(addr & 0xFF) as usize] = data;
                }
                true
            }
            0x7EF0..=0x7EFF => {
                match addr {
                    0x7EF0 => self.chr_banks[0] = data,
                    0x7EF1 => self.chr_banks[1] = data,
                    0x7EF2 => self.chr_banks[2] = data,
                    0x7EF3 => self.chr_banks[3] = data,
                    0x7EF4 => self.chr_banks[4] = data,
                    0x7EF5 => self.chr_banks[5] = data,
                    0x7EF6 => {
                        self.mirroring = if (data & 1) != 0 {
                            Mirroring::SPAGE1
                        } else {
                            Mirroring::SPAGE0
                        };
                    }
                    0x7EF8 => self.wram_enable = data,
                    0x7EFA | 0x7EFB => self.prg_banks[0] = data,
                    0x7EFC | 0x7EFD => self.prg_banks[1] = data,
                    0x7EFE | 0x7EFF => self.prg_banks[2] = data,
                    _ => {}
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
        let offset = addr as usize;
        let max_2k = self.chr_bank_count_2k();
        let max_1k = self.chr_bank_count_1k();
        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => {
                let (bank, bank_size, sub_offset, max_bank) = match offset {
                    0x0000..=0x07FF => (
                        (self.chr_banks[0] as usize >> 1) & 0x3F,
                        CHR_BANK_2K,
                        offset & 0x07FF,
                        max_2k,
                    ),
                    0x0800..=0x0FFF => (
                        (self.chr_banks[1] as usize >> 1) & 0x3F,
                        CHR_BANK_2K,
                        offset & 0x07FF,
                        max_2k,
                    ),
                    0x1000..=0x13FF => (
                        self.chr_banks[2] as usize,
                        CHR_BANK_1K,
                        offset & 0x03FF,
                        max_1k,
                    ),
                    0x1400..=0x17FF => (
                        self.chr_banks[3] as usize,
                        CHR_BANK_1K,
                        offset & 0x03FF,
                        max_1k,
                    ),
                    0x1800..=0x1BFF => (
                        self.chr_banks[4] as usize,
                        CHR_BANK_1K,
                        offset & 0x03FF,
                        max_1k,
                    ),
                    _ => (
                        self.chr_banks[5] as usize,
                        CHR_BANK_1K,
                        offset & 0x03FF,
                        max_1k,
                    ),
                };
                let bank = bank % max_bank;
                Some(chr_rom[(bank * bank_size + sub_offset) % chr_rom.len()])
            }
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset]),
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
        writer.write_bytes(&self.prg_banks);
        writer.write_bytes(&self.chr_banks);
        writer.write_u8(self.wram_enable);
        writer.write_u8(encode_mirroring(self.mirroring));
        writer.write_bytes(&self.wram);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_banks)?;
        reader.read_bytes_into(&mut self.chr_banks)?;
        self.wram_enable = reader.read_u8()?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        reader.read_bytes_into(&mut self.wram)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for Taito X1-005 save state",
                ));
            }
        }
        Ok(())
    }
}
