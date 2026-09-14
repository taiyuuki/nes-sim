use super::Mapper;
use crate::cartridge::Mirroring;
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_1K: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// Bandai LZ93D50 / FCG boards (iNES mappers 016 and 159).
///
/// Without NES 2.0 submapper info both register windows are honored: writes
/// at $6000-$7FFF behave like the FCG boards, writes at $8000-$FFFF like the
/// LZ93D50. The IRQ unit is a 16-bit counter clocked every CPU cycle.
pub(super) struct Bandai {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    chr_banks: [u8; 8],
    prg_bank: u8,
    mirroring: Mirroring,
    irq_latch: u16,
    irq_counter: u16,
    irq_enabled: bool,
    irq_line: bool,
    // One-shot IRQ pulse with a hold window for the CPU's sampling.
    irq_hold_cycles: u8,
}

impl Bandai {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            chr,
            chr_banks: [0; 8],
            prg_bank: 0,
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_line: false,
            irq_hold_cycles: 0,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_1K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_1K).max(1),
        }
    }

    fn write_register(&mut self, fcg_window: bool, addr: u16, data: u8) {
        match addr & 0x0F {
            0x00..=0x07 => self.chr_banks[(addr & 0x0F) as usize] = data,
            0x08 => self.prg_bank = data & 0x0F,
            0x09 => {
                self.mirroring = match data & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SPAGE0,
                    _ => Mirroring::SPAGE1,
                };
            }
            0x0A => {
                self.irq_counter = self.irq_latch;
                self.irq_enabled = (data & 0x01) != 0;
                self.irq_line = false;
            }
            0x0B | 0x0C => {
                let shift = 8 * ((addr & 0x0F) - 0x0B);
                if fcg_window {
                    // FCG boards poke the live counter instead of the latch.
                    self.irq_latch = (self.irq_latch & !(0xFF << shift)) | ((data as u16) << shift);
                    self.irq_counter =
                        (self.irq_counter & !(0xFF << shift)) | ((data as u16) << shift);
                } else {
                    self.irq_latch = (self.irq_latch & !(0xFF << shift)) | ((data as u16) << shift);
                }
            }
            _ => {}
        }
    }
}

impl Mapper for Bandai {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.prg_bank as usize % self.prg_bank_count_16k();
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
            0x6000..=0x7FFF => {
                self.write_register(true, addr, data);
                true
            }
            0x8000..=0xFFFF => {
                self.write_register(false, addr, data);
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
        if !self.irq_enabled {
            return;
        }
        if self.irq_counter == 0 {
            self.irq_line = true;
            self.irq_hold_cycles = 12;
            self.irq_counter = 0xFFFF;
        } else {
            self.irq_counter -= 1;
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
        writer.write_bytes(&self.chr_banks);
        writer.write_u8(self.prg_bank);
        writer.write_u8(encode_mirroring(self.mirroring));
        writer.write_u16(self.irq_latch);
        writer.write_u16(self.irq_counter);
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.irq_line);
        writer.write_u8(self.irq_hold_cycles);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for Bandai save state",
                ));
            }
        }
        reader.read_bytes_into(&mut self.chr_banks)?;
        self.prg_bank = reader.read_u8()?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        self.irq_latch = reader.read_u16()?;
        self.irq_counter = reader.read_u16()?;
        self.irq_enabled = reader.read_bool()?;
        self.irq_line = reader.read_bool()?;
        self.irq_hold_cycles = reader.read_u8()?;
        Ok(())
    }
}
