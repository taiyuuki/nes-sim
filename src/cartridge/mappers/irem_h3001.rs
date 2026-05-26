use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct IremH3001 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    prg_banks: [u8; 3],
    chr_banks: [u8; 8],
    irq_counter: u16,
    irq_reload: u16,
    irq_enabled: bool,
    mirror_ctrl: u8,
}

impl IremH3001 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };
        let mirror_ctrl = match mirroring {
            Mirroring::Vertical => 0x00,
            Mirroring::Horizontal => 0x80,
            _ => 0x00,
        };
        Self {
            prg_rom,
            chr,
            prg_banks: [0, 1, 2],
            chr_banks: [0; 8],
            irq_counter: 0,
            irq_reload: 0,
            irq_enabled: false,
            mirror_ctrl,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_8K
    }

    fn chr_bank_count_1k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_1K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_1K).max(1),
        }
    }
}

impl Mapper for IremH3001 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
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
            0x8000..=0x8FFF => {
                self.prg_banks[0] = data;
                true
            }
            0x9000 | 0x9001 => {
                self.mirror_ctrl = data;
                true
            }
            0x9003 => {
                self.irq_enabled = (data & 0x80) != 0;
                if !self.irq_enabled {
                    self.irq_counter = 0;
                }
                true
            }
            0x9004 => {
                self.irq_counter = self.irq_reload;
                self.irq_enabled = false;
                true
            }
            0x9005 => {
                self.irq_reload = (self.irq_reload & 0x00FF) | ((data as u16) << 8);
                true
            }
            0x9006 => {
                self.irq_reload = (self.irq_reload & 0xFF00) | data as u16;
                true
            }
            0xA000..=0xAFFF => {
                self.prg_banks[1] = data;
                true
            }
            0xB000..=0xBFFF => {
                self.chr_banks[(addr & 0x07) as usize] = data;
                true
            }
            0xC000..=0xCFFF => {
                self.prg_banks[2] = data;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let slot = addr as usize / CHR_BANK_1K;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_1k();
        let offset = bank * CHR_BANK_1K + (addr as usize & 0x03FF);
        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset % chr_ram.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let slot = addr as usize / CHR_BANK_1K;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_1k();
        let offset = bank * CHR_BANK_1K + (addr as usize & 0x03FF);
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            let len = chr_ram.len();
            chr_ram[offset % len] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        if self.mirror_ctrl & 0x80 != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn irq_line(&self) -> bool {
        self.irq_enabled && self.irq_counter == 0
    }

    fn tick_cpu_cycle(&mut self) {
        if !self.irq_enabled {
            return;
        }
        if self.irq_counter == 0 {
            self.irq_counter = self.irq_reload;
        } else {
            self.irq_counter = self.irq_counter.wrapping_sub(1);
        }
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.prg_banks);
        writer.write_bytes(&self.chr_banks);
        writer.write_u16(self.irq_counter);
        writer.write_u16(self.irq_reload);
        writer.write_bool(self.irq_enabled);
        writer.write_u8(self.mirror_ctrl);
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
        self.irq_counter = reader.read_u16()?;
        self.irq_reload = reader.read_u16()?;
        self.irq_enabled = reader.read_bool()?;
        self.mirror_ctrl = reader.read_u8()?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for Irem H-3001 save state",
                ));
            }
        }
        Ok(())
    }
}
