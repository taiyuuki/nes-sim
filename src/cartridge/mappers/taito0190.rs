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

pub(super) struct Taito0190 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    regs: [u8; 8],
    mirroring: Mirroring,
    is_48: bool,
    irq_latch: u8,
    irq_counter: u16,
    irq_enabled: bool,
}

impl Taito0190 {
    pub(super) fn new(
        prg_rom: Vec<u8>,
        chr_rom: Vec<u8>,
        mirroring: Mirroring,
        is_48: bool,
    ) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            chr,
            regs: [0; 8],
            mirroring,
            is_48,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
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

impl Mapper for Taito0190 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0x9FFF => {
                let bank = self.regs[0] as usize % self.prg_bank_count_8k();
                let offset = bank * PRG_BANK_8K + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xA000..=0xBFFF => {
                let bank = self.regs[1] as usize % self.prg_bank_count_8k();
                let offset = bank * PRG_BANK_8K + (addr as usize - 0xA000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xC000..=0xDFFF => {
                let second_last = self.prg_bank_count_8k().saturating_sub(2);
                let offset = second_last * PRG_BANK_8K + (addr as usize - 0xC000);
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
        if self.is_48 && addr >= 0xC000 {
            match addr & 0xF003 {
                0xC000 => {
                    self.irq_latch = data;
                }
                0xC001 => {
                    self.irq_counter = self.irq_latch as u16;
                }
                0xC002 => {
                    self.irq_enabled = true;
                }
                0xC003 => {
                    self.irq_enabled = false;
                }
                0xE000 => {
                    self.mirroring = if ((data >> 6) & 1) != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
                _ => return false,
            }
            return true;
        }

        match addr & 0xF003 {
            0x8000 => {
                self.regs[0] = data & 0x3F;
                if !self.is_48 {
                    self.mirroring = if ((data >> 6) & 1) != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
            }
            0x8001 => {
                self.regs[1] = data & 0x3F;
            }
            0x8002 => {
                self.regs[2] = data;
            }
            0x8003 => {
                self.regs[3] = data;
            }
            0xA000 => {
                self.regs[4] = data;
            }
            0xA001 => {
                self.regs[5] = data;
            }
            0xA002 => {
                self.regs[6] = data;
            }
            0xA003 => {
                self.regs[7] = data;
            }
            _ => return false,
        }
        true
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
                let bank = match offset {
                    0x0000..=0x07FF => self.regs[2] as usize % max_2k,
                    0x0800..=0x0FFF => self.regs[3] as usize % max_2k,
                    0x1000..=0x13FF => self.regs[4] as usize % max_1k,
                    0x1400..=0x17FF => self.regs[5] as usize % max_1k,
                    0x1800..=0x1BFF => self.regs[6] as usize % max_1k,
                    _ => self.regs[7] as usize % max_1k,
                };
                let base = match offset {
                    0x0000..=0x07FF => bank * CHR_BANK_2K,
                    0x0800..=0x0FFF => bank * CHR_BANK_2K,
                    _ => bank * CHR_BANK_1K,
                };
                let sub_offset = match offset {
                    0x0000..=0x0FFF => offset & 0x07FF,
                    _ => offset & 0x03FF,
                };
                Some(chr_rom[(base + sub_offset) % chr_rom.len()])
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

    fn irq_line(&self) -> bool {
        self.is_48 && self.irq_enabled && self.irq_counter == 0xFF
    }

    fn tick_cpu_cycle(&mut self) {
        if !self.is_48 || !self.irq_enabled {
            return;
        }
        self.irq_counter = self.irq_counter.wrapping_add(1);
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.regs);
        writer.write_bool(self.is_48);
        writer.write_u8(encode_mirroring(self.mirroring));
        if self.is_48 {
            writer.write_u8(self.irq_latch);
            writer.write_u16(self.irq_counter);
            writer.write_bool(self.irq_enabled);
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
        reader.read_bytes_into(&mut self.regs)?;
        self.is_48 = reader.read_bool()?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        if self.is_48 {
            self.irq_latch = reader.read_u8()?;
            self.irq_counter = reader.read_u16()?;
            self.irq_enabled = reader.read_bool()?;
        }
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for Taito TC0190 save state",
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
            "invalid Taito TC0190 mirroring value",
        )),
    }
}
