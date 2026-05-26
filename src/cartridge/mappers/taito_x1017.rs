use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_2K: usize = 0x0800;
const CHR_BANK_1K: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct TaitoX1017 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    regs: [u8; 9],
    mirroring: Mirroring,
    control: u8,
}

impl TaitoX1017 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        Self {
            prg_rom,
            chr,
            regs: [0, 2, 4, 5, 6, 7, 0, 1, 2],
            mirroring,
            control: 0,
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

impl Mapper for TaitoX1017 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0x9FFF => {
                let bank = self.regs[6] as usize % self.prg_bank_count_8k();
                let offset = bank * PRG_BANK_8K + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xA000..=0xBFFF => {
                let bank = self.regs[7] as usize % self.prg_bank_count_8k();
                let offset = bank * PRG_BANK_8K + (addr as usize - 0xA000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xC000..=0xDFFF => {
                let bank = self.regs[8] as usize % self.prg_bank_count_8k();
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
            0x7EF0..=0x7EF5 => {
                self.regs[(addr & 0x07) as usize] = data;
                true
            }
            0x7EF6 => {
                self.control = data & 0x03;
                self.mirroring = if (self.control & 0x01) != 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
                true
            }
            0x7EFA => {
                self.regs[6] = data >> 2;
                true
            }
            0x7EFB => {
                self.regs[7] = data >> 2;
                true
            }
            0x7EFC => {
                self.regs[8] = data >> 2;
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
        let swap = ((self.control & 0x02) as usize) << 11;

        let (_bank_size, base, sub) = match (offset ^ swap) & 0x1FFF {
            0x0000..=0x07FF => {
                let bank = (self.regs[0] as usize >> 1) % max_2k;
                (CHR_BANK_2K, bank * CHR_BANK_2K, offset & 0x07FF)
            }
            0x0800..=0x0FFF => {
                let bank = (self.regs[1] as usize >> 1) % max_2k;
                (CHR_BANK_2K, bank * CHR_BANK_2K, offset & 0x07FF)
            }
            0x1000..=0x13FF => {
                let bank = self.regs[2] as usize % max_1k;
                (CHR_BANK_1K, bank * CHR_BANK_1K, offset & 0x03FF)
            }
            0x1400..=0x17FF => {
                let bank = self.regs[3] as usize % max_1k;
                (CHR_BANK_1K, bank * CHR_BANK_1K, offset & 0x03FF)
            }
            0x1800..=0x1BFF => {
                let bank = self.regs[4] as usize % max_1k;
                (CHR_BANK_1K, bank * CHR_BANK_1K, offset & 0x03FF)
            }
            _ => {
                let bank = self.regs[5] as usize % max_1k;
                (CHR_BANK_1K, bank * CHR_BANK_1K, offset & 0x03FF)
            }
        };

        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[(base + sub) % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[(base + sub) % chr_ram.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let offset = addr as usize;
        let max_2k = self.chr_bank_count_2k();
        let max_1k = self.chr_bank_count_1k();
        let swap = ((self.control & 0x02) as usize) << 11;

        let (_bank_size, base, sub) = match (offset ^ swap) & 0x1FFF {
            0x0000..=0x07FF => {
                let bank = (self.regs[0] as usize >> 1) % max_2k;
                (CHR_BANK_2K, bank * CHR_BANK_2K, offset & 0x07FF)
            }
            0x0800..=0x0FFF => {
                let bank = (self.regs[1] as usize >> 1) % max_2k;
                (CHR_BANK_2K, bank * CHR_BANK_2K, offset & 0x07FF)
            }
            0x1000..=0x13FF => {
                let bank = self.regs[2] as usize % max_1k;
                (CHR_BANK_1K, bank * CHR_BANK_1K, offset & 0x03FF)
            }
            0x1400..=0x17FF => {
                let bank = self.regs[3] as usize % max_1k;
                (CHR_BANK_1K, bank * CHR_BANK_1K, offset & 0x03FF)
            }
            0x1800..=0x1BFF => {
                let bank = self.regs[4] as usize % max_1k;
                (CHR_BANK_1K, bank * CHR_BANK_1K, offset & 0x03FF)
            }
            _ => {
                let bank = self.regs[5] as usize % max_1k;
                (CHR_BANK_1K, bank * CHR_BANK_1K, offset & 0x03FF)
            }
        };

        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            let len = chr_ram.len();
            chr_ram[(base + sub) % len] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.regs);
        writer.write_u8(self.control);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.regs)?;
        self.control = reader.read_u8()?;
        self.mirroring = if (self.control & 0x01) != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for Taito X1-017 save state",
                ));
            }
        }
        Ok(())
    }
}
