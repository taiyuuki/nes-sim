use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_8K: usize = 0x2000;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// Konami VRC3 board (iNES mapper 073, Salamander).
///
/// PRG/CHR banking is minimal; the board's defining feature is its 16-bit
/// CPU-clocked IRQ counter with an 8-bit operating mode.
pub(super) struct Vrc3 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    prg_bank: u8,
    mirroring: Mirroring,
    irq_latch: u16,
    irq_counter: u16,
    irq_enabled: bool,
    irq_ack_enable: bool,
    irq_mode_8bit: bool,
    irq_pending: bool,
}

impl Vrc3 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; CHR_BANK_8K])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            chr,
            prg_bank: 0,
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_ack_enable: false,
            irq_mode_8bit: false,
            irq_pending: false,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn fire_irq(&mut self) {
        self.irq_pending = true;
        self.irq_counter = self.irq_latch;
    }
}

impl Mapper for Vrc3 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let bank = (self.prg_bank & 0x0F) as usize % self.prg_bank_count_16k();
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
            0x8000..=0x8FFF => {
                self.irq_latch = (self.irq_latch & 0xFFF0) | (data & 0x0F) as u16;
                true
            }
            0x9000..=0x9FFF => {
                self.irq_latch = (self.irq_latch & 0xFF0F) | ((data & 0x0F) as u16) << 4;
                true
            }
            0xA000..=0xAFFF => {
                self.irq_latch = (self.irq_latch & 0xF0FF) | ((data & 0x0F) as u16) << 8;
                true
            }
            0xB000..=0xBFFF => {
                self.irq_latch = (self.irq_latch & 0x0FFF) | ((data & 0x0F) as u16) << 12;
                true
            }
            0xC000..=0xCFFF => {
                self.irq_ack_enable = (data & 0x01) != 0;
                self.irq_mode_8bit = (data & 0x04) != 0;
                self.irq_pending = false;
                self.irq_enabled = (data & 0x02) != 0;
                if self.irq_enabled {
                    self.irq_counter = self.irq_latch;
                }
                true
            }
            0xD000..=0xDFFF => {
                self.irq_enabled = self.irq_ack_enable;
                self.irq_pending = false;
                true
            }
            0xF000..=0xFFFF => {
                self.prg_bank = data;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        match &self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[addr as usize % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[addr as usize]),
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
        self.irq_pending
    }

    fn tick_cpu_cycle(&mut self) {
        if !self.irq_enabled {
            return;
        }
        if self.irq_mode_8bit {
            let low = (self.irq_counter & 0x00FF) as u8;
            let (new_low, overflow) = low.overflowing_add(1);
            self.irq_counter = (self.irq_counter & 0xFF00) | new_low as u16;
            if overflow {
                self.fire_irq();
            }
        } else {
            self.irq_counter = self.irq_counter.wrapping_add(1);
            if self.irq_counter == 0 {
                self.fire_irq();
            }
        }
    }

    fn save_state(&self, writer: &mut StateWriter) {
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
        writer.write_u8(self.prg_bank);
        writer.write_u16(self.irq_latch);
        writer.write_u16(self.irq_counter);
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.irq_ack_enable);
        writer.write_bool(self.irq_mode_8bit);
        writer.write_bool(self.irq_pending);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for VRC3 save state",
                ));
            }
        }
        self.prg_bank = reader.read_u8()?;
        self.irq_latch = reader.read_u16()?;
        self.irq_counter = reader.read_u16()?;
        self.irq_enabled = reader.read_bool()?;
        self.irq_ack_enable = reader.read_bool()?;
        self.irq_mode_8bit = reader.read_bool()?;
        self.irq_pending = reader.read_bool()?;
        Ok(())
    }
}
