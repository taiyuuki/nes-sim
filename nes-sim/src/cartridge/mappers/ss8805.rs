use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// Jaleco SS8805 board (iNES mapper 018, Ninja Jajamaru - Ginga Daisakusen /
/// Goal!!). Bank registers are split across nibble-mirrored addresses, and
/// the IRQ offers several counter widths.
pub(super) struct Ss8805 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    prg_banks: [u8; 3],
    chr_banks: [u8; 8],
    mirroring: Mirroring,
    irq_latch: u16,
    irq_counter: u16,
    irq_enabled: bool,
    irq_line: bool,
    // The board raises a one-shot IRQ; hold it long enough for the CPU's
    // instruction-boundary sampling to observe the pulse.
    irq_hold_cycles: u8,
}

impl Ss8805 {
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
            prg_banks: [0, 1, 2],
            chr_banks: [0, 1, 2, 3, 4, 5, 4, 5],
            mirroring,
            irq_latch: 0xFFFF,
            irq_counter: 0xFFFF,
            irq_enabled: false,
            irq_line: false,
            irq_hold_cycles: 0,
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

    fn write_nibble_register(reg: &mut u8, addr: u16, data: u8) {
        if (addr & 1) != 0 {
            *reg = (*reg & 0x0F) | ((data & 0x0F) << 4);
        } else {
            *reg = (*reg & 0xF0) | (data & 0x0F);
        }
    }
}

impl Mapper for Ss8805 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0xDFFF => {
                let slot = ((addr - 0x8000) as usize) / PRG_BANK_8K;
                let bank = self.prg_banks[slot] as usize % self.prg_bank_count();
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
            0x6000..=0x7FFF => {
                self.prg_ram[(addr - 0x6000) as usize] = data;
                true
            }
            0x8000..=0x8003 => {
                let index = (addr as usize & 0x02) >> 1;
                Self::write_nibble_register(&mut self.prg_banks[index], addr, data);
                true
            }
            0x9000..=0x9001 => {
                Self::write_nibble_register(&mut self.prg_banks[2], addr, data);
                true
            }
            0xA000..=0xDFFF => {
                let base_slot = 2 * ((addr - 0xA000) as usize / 0x1000);
                let index = base_slot + (((addr as usize) & 0x02) >> 1);
                Self::write_nibble_register(&mut self.chr_banks[index], addr, data);
                true
            }
            0xE000..=0xE003 => {
                let shift = 4 * (addr & 0x03);
                self.irq_latch =
                    (self.irq_latch & !(0x000F << shift)) | (((data & 0x0F) as u16) << shift);
                true
            }
            0xF000..=0xF003 => {
                if (addr & 0x03) == 0x02 {
                    self.mirroring = match data & 0x03 {
                        0 => Mirroring::Horizontal,
                        1 => Mirroring::Vertical,
                        _ => Mirroring::FourScreen,
                    };
                } else if (addr & 0x03) == 0x01 {
                    // $F001 combines the latch high nibble with the enable
                    // bit in bit 4.
                    self.irq_enabled = (data & 0x10) != 0;
                }
                self.irq_counter = self.irq_latch;
                self.irq_line = false;
                self.irq_hold_cycles = 0;
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let bank = self.chr_banks[addr as usize / CHR_BANK_1K] as usize % self.chr_bank_count_1k();
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
        let bank = self.chr_banks[addr as usize / CHR_BANK_1K] as usize % self.chr_bank_count_1k();
        let index = bank * CHR_BANK_1K + (addr as usize & 0x03FF);
        if let ChrMemory::Ram(chr_ram) = &mut self.chr {
            chr_ram[index] = data;
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_line(&self) -> bool {
        self.irq_line
    }

    fn tick_cpu_cycle(&mut self) {
        if self.irq_line {
            self.irq_hold_cycles = self.irq_hold_cycles.saturating_sub(1);
            if self.irq_hold_cycles == 0 {
                self.irq_line = false;
            }
            return;
        }
        if !self.irq_enabled || self.irq_counter == 0 {
            return;
        }
        self.irq_counter -= 1;
        if self.irq_counter == 0 {
            self.irq_line = true;
            self.irq_hold_cycles = 12;
            self.irq_enabled = false;
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
        writer.write_bytes(&self.prg_banks);
        writer.write_bytes(&self.chr_banks);
        writer.write_u8(encode_mirroring(self.mirroring));
        writer.write_u16(self.irq_latch);
        writer.write_u16(self.irq_counter);
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.irq_line);
        writer.write_u8(self.irq_hold_cycles);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_ram)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for SS8805 save state",
                ));
            }
        }
        reader.read_bytes_into(&mut self.prg_banks)?;
        reader.read_bytes_into(&mut self.chr_banks)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        self.irq_latch = reader.read_u16()?;
        self.irq_counter = reader.read_u16()?;
        self.irq_enabled = reader.read_bool()?;
        self.irq_line = reader.read_bool()?;
        self.irq_hold_cycles = reader.read_u8()?;
        Ok(())
    }
}
