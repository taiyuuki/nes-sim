use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_LEN: usize = 0x4000;
const CHR_RAM_LEN: usize = 0x2000;

// The countdown runs for BASE_TIME * (DIP + 16) CPU cycles. DIP 4 is the
// cartridge's default 6:15 competition timer. Tests shrink the base so the
// expiry path can be exercised without millions of ticks.
#[cfg(not(test))]
const IRQ_TIME_BASE: u32 = 0x0200_0000;
#[cfg(test)]
const IRQ_TIME_BASE: u32 = 16;
const IRQ_DIP: u32 = 4;

/// NES-EVENT board (iNES mapper 105, Nintendo World Championships 1990).
/// An MMC1 whose CHR bank 0 lines double as PRG chip selects and a
/// competition countdown timer wired to the IRQ line.
pub(super) struct Mapper105 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr_ram: Vec<u8>,
    shift_register: u8,
    control: u8,
    chr_bank_0: u8,
    chr_bank_1: u8,
    prg_bank: u8,
    irq_count: u32,
    irq_line: bool,
}

impl Mapper105 {
    pub(super) fn new(prg_rom: Vec<u8>, _chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let mut control = 0x0C;
        if matches!(mirroring, Mirroring::FourScreen) {
            control &= !0x03;
        }
        Self {
            prg_rom,
            prg_ram: vec![0; PRG_RAM_LEN],
            chr_ram: vec![0; CHR_RAM_LEN],
            shift_register: 0x10,
            control,
            chr_bank_0: 0,
            chr_bank_1: 0,
            prg_bank: 0,
            irq_count: 0,
            irq_line: false,
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_LEN).max(1)
    }

    fn prg_bank_16k(&self, slot: usize) -> usize {
        let count = self.prg_bank_count();
        if self.chr_bank_0 & 0x08 != 0 {
            let bank = match self.control & 0x0C {
                0x00 | 0x04 => {
                    // 32 KiB mode: base 4 | (prg_bank >> 1 & 3) mirrored into
                    // both 16 KiB slots.
                    0x04 | (usize::from(self.prg_bank) >> 1 & 0x3)
                }
                0x08 => {
                    if slot == 0 {
                        0x08
                    } else {
                        0x08 | (usize::from(self.prg_bank) & 0x7)
                    }
                }
                _ => {
                    if slot == 0 {
                        0x08 | (usize::from(self.prg_bank) & 0x7)
                    } else {
                        0x0F
                    }
                }
            };
            bank % count
        } else {
            // 32 KiB mode selecting from the lower PRG chip.
            let bank32 = (usize::from(self.chr_bank_0) >> 1) & 0x3;
            (bank32 * 2 + slot) % count
        }
    }

    fn update_irq_on_commit(&mut self) {
        if self.chr_bank_0 & 0x10 != 0 {
            self.irq_count = 0;
            self.irq_line = false;
        } else if self.irq_count == 0 {
            self.irq_count = IRQ_TIME_BASE * (IRQ_DIP + 16) - 1;
        }
    }
}

impl Mapper for Mapper105 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0xFFFF => {
                let slot = if addr < 0xC000 { 0 } else { 1 };
                let bank = self.prg_bank_16k(slot);
                Some(self.prg_rom[bank * PRG_BANK_LEN + (addr as usize & 0x3FFF)])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x6000..=0x7FFF => {
                self.prg_ram[(addr - 0x6000) as usize] = data;
                true
            }
            0x8000..=0xFFFF => {
                if data & 0x80 != 0 {
                    self.shift_register = 0x10;
                    self.control |= 0x0C;
                    return true;
                }

                let complete = (self.shift_register & 0x01) != 0;
                self.shift_register >>= 1;
                self.shift_register |= (data & 0x01) << 4;

                if complete {
                    let value = self.shift_register;
                    self.shift_register = 0x10;
                    match addr {
                        0x8000..=0x9FFF => self.control = value & 0x1F,
                        0xA000..=0xBFFF => {
                            self.chr_bank_0 = value & 0x1F;
                            self.update_irq_on_commit();
                        }
                        0xC000..=0xDFFF => self.chr_bank_1 = value & 0x1F,
                        _ => {
                            self.prg_bank = value & 0x1F;
                            self.update_irq_on_commit();
                        }
                    }
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
        Some(self.chr_ram[addr as usize])
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        self.chr_ram[addr as usize] = data;
        true
    }

    fn mirroring(&self) -> Mirroring {
        match self.control & 0x03 {
            0 => Mirroring::SPAGE0,
            1 => Mirroring::SPAGE1,
            2 => Mirroring::Vertical,
            _ => Mirroring::Horizontal,
        }
    }

    fn tick_cpu_cycle(&mut self) {
        if self.irq_count > 0 {
            self.irq_count -= 1;
            if self.irq_count == 0 {
                self.irq_line = true;
            }
        }
    }

    fn irq_line(&self) -> bool {
        self.irq_line
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.prg_ram);
        writer.write_bytes(&self.chr_ram);
        writer.write_u8(self.shift_register);
        writer.write_u8(self.control);
        writer.write_u8(self.chr_bank_0);
        writer.write_u8(self.chr_bank_1);
        writer.write_u8(self.prg_bank);
        writer.write_u32(self.irq_count);
        writer.write_bool(self.irq_line);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_ram)?;
        reader.read_bytes_into(&mut self.chr_ram)?;
        self.shift_register = reader.read_u8()?;
        self.control = reader.read_u8()?;
        self.chr_bank_0 = reader.read_u8()?;
        self.chr_bank_1 = reader.read_u8()?;
        self.prg_bank = reader.read_u8()?;
        self.irq_count = reader.read_u32()?;
        self.irq_line = reader.read_bool()?;
        Ok(())
    }
}
