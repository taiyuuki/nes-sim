use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;

// The RAMBO-1 scans its IRQ counter on PPU A12 rising edges that follow a
// sufficiently long low period, just like the MMC3.
const A12_LOW_FILTER_PPU_CYCLES: u64 = 10;
// In CPU clock mode the counter runs off M2 divided by four.
const CPU_MODE_DIVIDER: u32 = 4;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// Tengen RAMBO-1 board (iNES mapper 064, Tengen's MMC3 clone).
pub(super) struct Rambo1 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    ctrl: u8,
    chr_regs: [u8; 8],
    prg_regs: [u8; 3],
    mirroring: Mirroring,
    irq_latch: u8,
    irq_counter: u8,
    irq_reload_pending: bool,
    irq_enabled: bool,
    irq_line: bool,
    irq_cpu_mode: bool,
    cpu_mode_divider: u32,
    rendering_active: bool,
    last_a12: bool,
    a12_fall_cycle: u64,
}

impl Rambo1 {
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
            ctrl: 0,
            chr_regs: [0; 8],
            prg_regs: [0, 1, 2],
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_reload_pending: false,
            irq_enabled: false,
            irq_line: false,
            irq_cpu_mode: false,
            cpu_mode_divider: 0,
            rendering_active: false,
            last_a12: false,
            a12_fall_cycle: 0,
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_8K).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_1K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_1K).max(1),
        }
    }

    fn prg_bank_number(&self, slot: usize) -> usize {
        let count = self.prg_bank_count();
        let last = count - 1;
        if slot == 3 {
            return last;
        }
        let bank = match (self.ctrl & 0x40 != 0, slot) {
            (false, 0) | (true, 1) => self.prg_regs[0],
            (false, 1) | (true, 2) => self.prg_regs[1],
            (false, 2) | (true, 0) => self.prg_regs[2],
            _ => unreachable!(),
        } as usize;
        bank % count
    }

    fn chr_bank_number(&self, slot: usize) -> usize {
        let bank_count = self.chr_bank_count_1k();
        let group_a_base = usize::from(self.ctrl & 0x80 != 0) * 4;
        let slot_in_half = slot % 4;
        let is_group_a = slot / 4 == group_a_base / 4;

        let bank = if is_group_a {
            if self.ctrl & 0x20 != 0 {
                match slot_in_half {
                    0 => self.chr_regs[0],
                    1 => self.chr_regs[6],
                    2 => self.chr_regs[1],
                    _ => self.chr_regs[7],
                }
            } else {
                (match slot_in_half / 2 {
                    0 => self.chr_regs[0] & 0xFE,
                    _ => self.chr_regs[1] & 0xFE,
                }) + (slot_in_half as u8 % 2)
            }
        } else {
            self.chr_regs[2 + slot_in_half]
        };
        (bank as usize) % bank_count
    }

    fn clock_irq_counter(&mut self) {
        if self.irq_latch == 1 {
            self.irq_counter = 0;
        } else if self.irq_reload_pending {
            self.irq_reload_pending = false;
            self.irq_counter = self.irq_latch | u8::from(self.irq_latch != 0);
            if self.irq_cpu_mode {
                self.irq_counter |= 2;
            }
        } else if self.irq_counter == 0 {
            self.irq_counter = self.irq_latch;
        } else {
            self.irq_counter -= 1;
        }

        if self.irq_counter == 0 && self.irq_enabled {
            self.irq_line = true;
        }
    }
}

impl Mapper for Rambo1 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0xFFFF => {
                let slot = ((addr - 0x8000) as usize) / PRG_BANK_8K;
                let bank = self.prg_bank_number(slot);
                Some(self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)])
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
                    match self.ctrl & 0x0F {
                        0..=5 => self.chr_regs[(self.ctrl & 0x0F) as usize] = data,
                        6 | 7 => self.prg_regs[(self.ctrl as usize & 0x0F) - 6] = data,
                        8 | 9 => self.chr_regs[(self.ctrl as usize & 0x0F) - 2] = data,
                        0x0F => self.prg_regs[2] = data,
                        _ => {}
                    }
                } else {
                    self.ctrl = data;
                }
                true
            }
            0xA000..=0xBFFF => {
                if !odd && !matches!(self.mirroring, Mirroring::FourScreen) {
                    self.mirroring = if (data & 0x01) != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
                true
            }
            0xC000..=0xDFFF => {
                if odd {
                    self.irq_reload_pending = true;
                    self.irq_cpu_mode = (data & 0x01) != 0;
                } else {
                    self.irq_latch = data;
                }
                true
            }
            0xE000..=0xFFFF => {
                if odd {
                    self.irq_enabled = true;
                } else {
                    self.irq_enabled = false;
                    self.irq_line = false;
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
        if self.irq_cpu_mode || !self.rendering_active {
            return;
        }
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

    fn notify_scanline(&mut self, scanline: i16, rendering_on: bool) {
        // While rendering is off the PPU emits no fetches, so the A12 line
        // never rises and the scanline counter would stall. Games such as
        // Rolling Thunder rely on the counter still running during forced
        // blanking, so fall back to one clock per scanline then.
        self.rendering_active = rendering_on;
        if self.irq_cpu_mode || rendering_on {
            return;
        }
        if scanline > 239 && scanline != 261 {
            return;
        }
        self.clock_irq_counter();
    }

    fn irq_line(&self) -> bool {
        self.irq_line
    }

    fn tick_cpu_cycle(&mut self) {
        if !self.irq_enabled || !self.irq_cpu_mode {
            return;
        }
        self.cpu_mode_divider += 1;
        if self.cpu_mode_divider >= CPU_MODE_DIVIDER {
            self.cpu_mode_divider = 0;
            self.clock_irq_counter();
        }
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
        writer.write_u8(self.ctrl);
        writer.write_bytes(&self.chr_regs);
        writer.write_bytes(&self.prg_regs);
        writer.write_u8(encode_mirroring(self.mirroring));
        writer.write_u8(self.irq_latch);
        writer.write_u8(self.irq_counter);
        writer.write_bool(self.irq_reload_pending);
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.irq_line);
        writer.write_bool(self.irq_cpu_mode);
        writer.write_u32(self.cpu_mode_divider);
        writer.write_bool(self.rendering_active);
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
                    "CHR RAM mismatch for RAMBO-1 save state",
                ));
            }
        }
        self.ctrl = reader.read_u8()?;
        reader.read_bytes_into(&mut self.chr_regs)?;
        reader.read_bytes_into(&mut self.prg_regs)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        self.irq_latch = reader.read_u8()?;
        self.irq_counter = reader.read_u8()?;
        self.irq_reload_pending = reader.read_bool()?;
        self.irq_enabled = reader.read_bool()?;
        self.irq_line = reader.read_bool()?;
        self.irq_cpu_mode = reader.read_bool()?;
        self.cpu_mode_divider = reader.read_u32()?;
        self.rendering_active = reader.read_bool()?;
        self.last_a12 = reader.read_bool()?;
        self.a12_fall_cycle = reader.read_u64()?;
        Ok(())
    }
}
