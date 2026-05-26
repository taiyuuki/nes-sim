use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const CHR_RAM_LEN: usize = 0x4000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Namco163 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    chr_ram: Vec<u8>,
    chr_banks: [u8; 8],
    prg_banks: [u8; 3],
    chr_ram_enable_lo: bool,
    chr_ram_enable_hi: bool,
    irq_counter: u16,
    irq_enabled: bool,
    mirroring: Mirroring,
}

impl Namco163 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            chr,
            chr_ram: vec![0; CHR_RAM_LEN],
            chr_banks: [0; 8],
            prg_banks: [0, 1, 2],
            chr_ram_enable_lo: false,
            chr_ram_enable_hi: false,
            irq_counter: 0,
            irq_enabled: false,
            mirroring,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_8K
    }

    fn chr_bank_count_1k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(chr_rom) => (chr_rom.len() / CHR_BANK_1K).max(1),
            ChrMemory::Ram(chr_ram) => (chr_ram.len() / CHR_BANK_1K).max(1),
        }
    }
}

impl Mapper for Namco163 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x5000..=0x57FF => Some((self.irq_counter & 0xFF) as u8),
            0x5800..=0x5FFF => {
                let hi = ((self.irq_counter >> 8) as u8) & 0x7F;
                let flag = if self.irq_enabled { 0x80 } else { 0 };
                Some(hi | flag)
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
            0x5000..=0x57FF => {
                self.irq_counter = (self.irq_counter & 0xFF00) | data as u16;
                self.irq_enabled = false;
                true
            }
            0x5800..=0x5FFF => {
                self.irq_counter = (self.irq_counter & 0x00FF) | (((data & 0x7F) as u16) << 8);
                self.irq_enabled = (data & 0x80) != 0;
                true
            }
            0x8000..=0xBFFF => {
                let bank = (addr >> 11) as usize & 0x07;
                self.chr_banks[bank] = data;
                true
            }
            0xE000..=0xE7FF => {
                self.prg_banks[0] = data & 0x3F;
                true
            }
            0xE800..=0xEFFF => {
                self.prg_banks[1] = data & 0x3F;
                self.chr_ram_enable_lo = (data & 0x40) == 0;
                self.chr_ram_enable_hi = (data & 0x80) == 0;
                true
            }
            0xF000..=0xF7FF => {
                self.prg_banks[2] = data & 0x3F;
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
        let bank = self.chr_banks[slot] as usize;
        let offset = addr as usize & 0x03FF;

        if bank > 0xE0 {
            let is_hi = addr >= 0x1000;
            let enabled = if is_hi {
                self.chr_ram_enable_hi
            } else {
                self.chr_ram_enable_lo
            };
            if enabled {
                let ram_offset = (bank - 0xE0) * CHR_BANK_1K + offset;
                return Some(self.chr_ram[ram_offset % CHR_RAM_LEN]);
            }
        }

        let bank = bank % self.chr_bank_count_1k();
        let offset = bank * CHR_BANK_1K + offset;
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
        let bank = self.chr_banks[slot] as usize;
        let offset = addr as usize & 0x03FF;

        if bank > 0xE0 {
            let is_hi = addr >= 0x1000;
            let enabled = if is_hi {
                self.chr_ram_enable_hi
            } else {
                self.chr_ram_enable_lo
            };
            if enabled {
                let ram_offset = (bank - 0xE0) * CHR_BANK_1K + offset;
                self.chr_ram[ram_offset % CHR_RAM_LEN] = data;
                return true;
            }
        }

        let bank = bank % self.chr_bank_count_1k();
        let offset = bank * CHR_BANK_1K + offset;
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            let len = chr_ram.len();
            chr_ram[offset % len] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_line(&self) -> bool {
        self.irq_enabled && self.irq_counter >= 0x7FFF
    }

    fn tick_cpu_cycle(&mut self) {
        self.irq_counter = self.irq_counter.wrapping_add(1);
        if self.irq_counter > 0x7FFF {
            self.irq_counter = 0x7FFF;
        }
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.chr_banks);
        writer.write_bytes(&self.prg_banks);
        writer.write_bool(self.chr_ram_enable_lo);
        writer.write_bool(self.chr_ram_enable_hi);
        writer.write_u16(self.irq_counter);
        writer.write_bool(self.irq_enabled);
        writer.write_u8(encode_mirroring(self.mirroring));
        writer.write_bytes(&self.chr_ram);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.chr_banks)?;
        reader.read_bytes_into(&mut self.prg_banks)?;
        self.chr_ram_enable_lo = reader.read_bool()?;
        self.chr_ram_enable_hi = reader.read_bool()?;
        self.irq_counter = reader.read_u16()?;
        self.irq_enabled = reader.read_bool()?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        reader.read_bytes_into(&mut self.chr_ram)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for Namco 163 save state",
                ));
            }
        }
        Ok(())
    }
}

fn encode_mirroring(mirroring: Mirroring) -> u8 {
    match mirroring {
        Mirroring::Horizontal => 0,
        Mirroring::Vertical => 1,
        Mirroring::FourScreen => 2,
        Mirroring::SPAGE0 => 3,
        Mirroring::SPAGE1 => 4,
    }
}

fn decode_mirroring(encoded: u8) -> Result<Mirroring, SaveStateError> {
    match encoded {
        0 => Ok(Mirroring::Horizontal),
        1 => Ok(Mirroring::Vertical),
        2 => Ok(Mirroring::FourScreen),
        3 => Ok(Mirroring::SPAGE0),
        4 => Ok(Mirroring::SPAGE1),
        _ => Err(SaveStateError::InvalidData(
            "invalid Namco 163 mirroring value",
        )),
    }
}
