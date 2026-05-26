use super::Mapper;
use crate::cartridge::Mirroring;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_2K: usize = 0x0800;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Sunsoft3 {
    prg_rom: Vec<u8>,
    chr: ChrMemory,
    prg_bank: u8,
    chr_banks: [u8; 4],
    mirroring: Mirroring,
    irq_latch: u16,
    irq_counter: u16,
    irq_enabled: bool,
    toggle: bool,
}

impl Sunsoft3 {
    pub(super) fn new(prg_rom: Vec<u8>, chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            chr,
            prg_bank: 0,
            chr_banks: [0; 4],
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_enabled: false,
            toggle: false,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        self.prg_rom.len() / PRG_BANK_16K
    }

    fn chr_bank_count_2k(&self) -> usize {
        match &self.chr {
            ChrMemory::Rom(r) => (r.len() / CHR_BANK_2K).max(1),
            ChrMemory::Ram(r) => (r.len() / CHR_BANK_2K).max(1),
        }
    }
}

impl Mapper for Sunsoft3 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.prg_bank as usize % self.prg_bank_count_16k();
                let offset = bank * PRG_BANK_16K + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xC000..=0xFFFF => {
                let last = self.prg_bank_count_16k().saturating_sub(1);
                let offset = last * PRG_BANK_16K + (addr as usize - 0xC000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr & 0xF800 {
            0x8800 => {
                self.chr_banks[0] = data;
            }
            0x9800 => {
                self.chr_banks[1] = data;
            }
            0xA800 => {
                self.chr_banks[2] = data;
            }
            0xB800 => {
                self.chr_banks[3] = data;
            }
            0xC000 | 0xC800 => {
                if self.toggle {
                    self.irq_latch = (self.irq_latch & 0xFF) | ((data as u16) << 8);
                } else {
                    self.irq_latch = (self.irq_latch & 0xFF00) | data as u16;
                }
                self.toggle = !self.toggle;
            }
            0xD800 | 0xD000 => {
                self.toggle = false;
                self.irq_enabled = (data & 0x10) != 0;
            }
            0xE800 => {
                self.mirroring = match data & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SPAGE0,
                    3 => Mirroring::SPAGE1,
                    _ => unreachable!(),
                };
            }
            0xF800 => {
                self.prg_bank = data;
            }
            _ => return false,
        }
        true
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let slot = addr as usize / CHR_BANK_2K;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_2k();
        let offset = bank * CHR_BANK_2K + (addr as usize & 0x07FF);
        match &mut self.chr {
            ChrMemory::Rom(chr_rom) => Some(chr_rom[offset % chr_rom.len()]),
            ChrMemory::Ram(chr_ram) => Some(chr_ram[offset % chr_ram.len()]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return false;
        }
        let slot = addr as usize / CHR_BANK_2K;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_2k();
        let offset = bank * CHR_BANK_2K + (addr as usize & 0x07FF);
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
        self.irq_enabled && self.irq_counter == 0
    }

    fn tick_cpu_cycle(&mut self) {
        if !self.irq_enabled {
            return;
        }
        if self.irq_counter == 0 {
            self.irq_counter = self.irq_latch;
        } else {
            self.irq_counter = self.irq_counter.wrapping_sub(1);
        }
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.prg_bank);
        writer.write_bytes(&self.chr_banks);
        writer.write_u8(encode_mirroring(self.mirroring));
        writer.write_u16(self.irq_latch);
        writer.write_u16(self.irq_counter);
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.toggle);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.prg_bank = reader.read_u8()?;
        reader.read_bytes_into(&mut self.chr_banks)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        self.irq_latch = reader.read_u16()?;
        self.irq_counter = reader.read_u16()?;
        self.irq_enabled = reader.read_bool()?;
        self.toggle = reader.read_bool()?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for Sunsoft 3 save state",
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
            "invalid Sunsoft 3 mirroring value",
        )),
    }
}
