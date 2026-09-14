use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;

// Like the MMC3, this board only advances its scanline counter on PPU A12
// rising edges that follow a long enough low period.
const A12_LOW_FILTER_PPU_CYCLES: u64 = 10;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// Pirate MMC3-style board (iNES mapper 182, Super Donkey Kong). Register
/// bits and even/odd decoding are scrambled relative to a real MMC3.
pub(super) struct Mapper182 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    which_bank: u8,
    prg_config: bool,
    chr_config: bool,
    bank6: u8,
    bank_a000: u8,
    chr_regs: [u8; 6],
    mirroring: Mirroring,
    irq_reload_value: u8,
    irq_counter: u8,
    irq_reload_pending: bool,
    irq_enabled: bool,
    irq_line: bool,
    last_a12: bool,
    a12_fall_cycle: u64,
}

impl Mapper182 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            prg_ram: vec![0; PRG_RAM_LEN],
            chr,
            which_bank: 0,
            prg_config: false,
            chr_config: false,
            bank6: 0,
            bank_a000: 1,
            chr_regs: [0; 6],
            mirroring,
            irq_reload_value: 0,
            irq_counter: 0,
            irq_reload_pending: false,
            irq_enabled: false,
            irq_line: false,
            last_a12: false,
            a12_fall_cycle: 0,
        }
    }

    fn prg_bank_count(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_8K
    }

    fn chr_bank_count_1k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_1K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_1K).max(1),
        }
    }

    fn prg_bank_number(&self, slot: usize) -> usize {
        let count = self.prg_bank_count();
        let third_last = count.saturating_sub(2);
        let last = count - 1;
        match (self.prg_config, slot) {
            (false, 0) => (self.bank6 as usize) % count,
            (false, 2) => third_last,
            (true, 0) => third_last,
            (true, 2) => (self.bank6 as usize) % count,
            (_, 3) => last,
            _ => unreachable!(),
        }
    }

    fn chr_bank_number(&self, slot: usize) -> usize {
        let bank = if slot < 4 {
            if self.chr_config {
                // 1 KiB banks at $0000-$0FFF, 2 KiB pairs at $1000-$1FFF.
                self.chr_regs[2 + slot]
            } else {
                (match slot / 2 {
                    0 => self.chr_regs[0] & 0xFE,
                    _ => self.chr_regs[1] & 0xFE,
                }) + (slot as u8 & 1)
            }
        } else if self.chr_config {
            let pair = match (slot - 4) / 2 {
                0 => self.chr_regs[0] & 0xFE,
                _ => self.chr_regs[1] & 0xFE,
            };
            pair + ((slot as u8 - 4) & 1)
        } else {
            self.chr_regs[slot - 2]
        };
        (bank as usize) % self.chr_bank_count_1k()
    }

    fn clock_irq_counter(&mut self) {
        if self.irq_reload_pending {
            self.irq_reload_pending = false;
            self.irq_counter = self.irq_reload_value;
        }
        if self.irq_counter == 0 {
            if self.irq_reload_value == 0 {
                self.irq_counter = self.irq_counter.wrapping_sub(1);
                return;
            }
            if self.irq_enabled {
                self.irq_line = true;
            }
            self.irq_counter = self.irq_reload_value;
        } else {
            self.irq_counter -= 1;
        }
    }
}

impl Mapper for Mapper182 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0xDFFF => {
                let slot = ((addr - 0x8000) as usize) / PRG_BANK_8K;
                let bank = match slot {
                    1 => (self.bank_a000 as usize) % self.prg_bank_count(),
                    _ => self.prg_bank_number(slot),
                };
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
        let odd = (addr & 1) != 0;
        match addr {
            0x6000..=0x7FFF => {
                self.prg_ram[(addr - 0x6000) as usize] = data;
                true
            }
            0x8000..=0x9FFF => {
                if odd {
                    self.mirroring = if (data & 0x01) != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
                true
            }
            0xA000..=0xBFFF => {
                if !odd {
                    self.which_bank = data & 0x07;
                    self.prg_config = (data & 0x10) != 0;
                    self.chr_config = (data & 0x20) != 0;
                }
                true
            }
            0xC000..=0xDFFF => {
                if odd {
                    self.irq_reload_pending = true;
                    self.irq_reload_value = data;
                } else {
                    match self.which_bank {
                        0 => self.chr_regs[0] = data,
                        1 => self.chr_regs[3] = data,
                        2 => self.chr_regs[1] = data,
                        3 => self.chr_regs[5] = data,
                        4 => self.bank6 = data,
                        5 => self.bank_a000 = data,
                        6 => self.chr_regs[2] = data,
                        _ => self.chr_regs[4] = data,
                    }
                }
                true
            }
            0xE000..=0xFFFF => {
                if odd {
                    self.irq_enabled = true;
                } else {
                    self.irq_enabled = false;
                    self.irq_line = false;
                    self.irq_counter = self.irq_reload_value;
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
        let bank = self.chr_bank_number(addr as usize / CHR_BANK_1K);
        let index = bank * CHR_BANK_1K + (addr as usize & 0x03FF);
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[index]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[index]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let bank = self.chr_bank_number(addr as usize / CHR_BANK_1K);
        let index = bank * CHR_BANK_1K + (addr as usize & 0x03FF);
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            chr_ram[index] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn check_a12(&mut self, addr: u16, ppu_cycle: u64) {
        let a12 = (addr & 0x1000) != 0;
        if !a12 && self.last_a12 {
            self.a12_fall_cycle = ppu_cycle;
        } else if a12 && !self.last_a12 {
            let low_span = ppu_cycle.saturating_sub(self.a12_fall_cycle);
            if low_span >= A12_LOW_FILTER_PPU_CYCLES {
                self.clock_irq_counter();
            }
        }
        self.last_a12 = a12;
    }

    fn irq_line(&self) -> bool {
        self.irq_line
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.prg_ram);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
        writer.write_u8(self.which_bank);
        writer.write_bool(self.prg_config);
        writer.write_bool(self.chr_config);
        writer.write_u8(self.bank6);
        writer.write_u8(self.bank_a000);
        writer.write_bytes(&self.chr_regs);
        writer.write_u8(encode_mirroring(self.mirroring));
        writer.write_u8(self.irq_reload_value);
        writer.write_u8(self.irq_counter);
        writer.write_bool(self.irq_reload_pending);
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.irq_line);
        writer.write_bool(self.last_a12);
        writer.write_u64(self.a12_fall_cycle);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_ram)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for mapper 182 save state",
                ));
            }
        }
        self.which_bank = reader.read_u8()?;
        self.prg_config = reader.read_bool()?;
        self.chr_config = reader.read_bool()?;
        self.bank6 = reader.read_u8()?;
        self.bank_a000 = reader.read_u8()?;
        reader.read_bytes_into(&mut self.chr_regs)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        self.irq_reload_value = reader.read_u8()?;
        self.irq_counter = reader.read_u8()?;
        self.irq_reload_pending = reader.read_bool()?;
        self.irq_enabled = reader.read_bool()?;
        self.irq_line = reader.read_bool()?;
        self.last_a12 = reader.read_bool()?;
        self.a12_fall_cycle = reader.read_u64()?;
        Ok(())
    }
}
