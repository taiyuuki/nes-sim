use std::cell::RefCell;
use std::rc::Rc;

use super::Mapper;
use crate::apu::ExpansionAudioChip;
use crate::cartridge::Mirroring;
use crate::cartridge::expansion_audio::sunsoft5b::{Sunsoft5bAudio, Sunsoft5bAudioChip};
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_BANK_8K: usize = 0x2000;
const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Fme7 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    command: u8,
    chr_banks: [u8; 8],
    prg_bank_8: u8,
    prg_bank_9: u8,
    prg_bank_a: u8,
    prg_bank_b: u8,
    irq_counter: u16,
    irq_enabled: bool,
    irq_clock: bool,
    irq_pending: bool,
    mirroring: Mirroring,
    audio: Rc<RefCell<Sunsoft5bAudio>>,
}

impl Fme7 {
    fn new(
        prg_rom: Vec<u8>,
        chr_rom: Vec<u8>,
        mirroring: Mirroring,
        audio: Rc<RefCell<Sunsoft5bAudio>>,
    ) -> Self {
        let chr = if chr_rom.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_rom)
        };

        Self {
            prg_rom,
            prg_ram: vec![0; PRG_RAM_BANK_8K],
            chr,
            command: 0,
            chr_banks: [0; 8],
            prg_bank_8: 0,
            prg_bank_9: 0,
            prg_bank_a: 1,
            prg_bank_b: 2,
            irq_counter: 0xFFFF,
            irq_enabled: false,
            irq_clock: false,
            irq_pending: false,
            mirroring,
            audio,
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

    fn write_register(&mut self, data: u8) {
        match self.command {
            0x0..=0x7 => {
                self.chr_banks[self.command as usize] = data;
            }
            0x8 => {
                self.prg_bank_8 = data;
            }
            0x9 => {
                self.prg_bank_9 = data;
            }
            0xA => {
                self.prg_bank_a = data;
            }
            0xB => {
                self.prg_bank_b = data;
            }
            0xC => {
                self.mirroring = match data & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SPAGE0,
                    3 => Mirroring::SPAGE1,
                    _ => unreachable!(),
                };
            }
            0xD => {
                self.irq_clock = (data & 0x80) != 0;
                self.irq_enabled = (data & 0x01) != 0;
                self.irq_pending = false;
            }
            0xE => {
                self.irq_counter = (self.irq_counter & 0xFF00) | data as u16;
            }
            0xF => {
                self.irq_counter = (self.irq_counter & 0x00FF) | ((data as u16) << 8);
            }
            _ => {}
        }
    }
}

impl Mapper for Fme7 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => {
                let ram_select = (self.prg_bank_8 & 0x40) != 0;
                let ram_enabled = (self.prg_bank_8 & 0x80) != 0;
                if ram_select && ram_enabled {
                    let bank = (self.prg_bank_8 & 0x3F) as usize;
                    let offset = bank * PRG_RAM_BANK_8K + (addr as usize - 0x6000);
                    if offset < self.prg_ram.len() {
                        Some(self.prg_ram[offset])
                    } else {
                        Some((addr >> 8) as u8)
                    }
                } else {
                    let bank = (self.prg_bank_8 & 0x3F) as usize % self.prg_bank_count_8k();
                    let offset = bank * PRG_BANK_8K + (addr as usize - 0x6000);
                    Some(self.prg_rom[offset % self.prg_rom.len()])
                }
            }
            0x8000..=0x9FFF => {
                let bank = self.prg_bank_9 as usize % self.prg_bank_count_8k();
                let offset = bank * PRG_BANK_8K + (addr as usize - 0x8000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xA000..=0xBFFF => {
                let bank = self.prg_bank_a as usize % self.prg_bank_count_8k();
                let offset = bank * PRG_BANK_8K + (addr as usize - 0xA000);
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xC000..=0xDFFF => {
                let bank = self.prg_bank_b as usize % self.prg_bank_count_8k();
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
            0x6000..=0x7FFF => {
                let ram_select = (self.prg_bank_8 & 0x40) != 0;
                let ram_enabled = (self.prg_bank_8 & 0x80) != 0;
                if ram_select && ram_enabled {
                    let bank = (self.prg_bank_8 & 0x3F) as usize;
                    let offset = bank * PRG_RAM_BANK_8K + (addr as usize - 0x6000);
                    if offset < self.prg_ram.len() {
                        self.prg_ram[offset] = data;
                    }
                }
                true
            }
            0x8000..=0x9FFF => {
                self.command = data & 0x0F;
                true
            }
            0xA000..=0xBFFF => {
                self.write_register(data);
                true
            }
            0xC000..=0xDFFF => {
                self.audio.borrow_mut().write_address(data);
                true
            }
            0xE000..=0xFFFF => {
                self.audio.borrow_mut().write_data(data);
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
        self.mirroring
    }

    fn irq_line(&self) -> bool {
        self.irq_pending
    }

    fn tick_cpu_cycle(&mut self) {
        if !self.irq_clock {
            return;
        }
        if self.irq_counter == 0 {
            self.irq_counter = 0xFFFF;
        } else {
            self.irq_counter = self.irq_counter.wrapping_sub(1);
            if self.irq_counter == 0 && self.irq_enabled {
                self.irq_pending = true;
            }
        }
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.command);
        writer.write_bytes(&self.chr_banks);
        writer.write_u8(self.prg_bank_8);
        writer.write_u8(self.prg_bank_9);
        writer.write_u8(self.prg_bank_a);
        writer.write_u8(self.prg_bank_b);
        writer.write_u16(self.irq_counter);
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.irq_clock);
        writer.write_bool(self.irq_pending);
        writer.write_u8(encode_mirroring(self.mirroring));
        writer.write_bytes(&self.prg_ram);
        match &self.chr {
            ChrMemory::Rom(_) => writer.write_bool(false),
            ChrMemory::Ram(chr_ram) => {
                writer.write_bool(true);
                writer.write_bytes(chr_ram);
            }
        }
        self.audio.borrow().save_state(writer);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.command = reader.read_u8()?;
        reader.read_bytes_into(&mut self.chr_banks)?;
        self.prg_bank_8 = reader.read_u8()?;
        self.prg_bank_9 = reader.read_u8()?;
        self.prg_bank_a = reader.read_u8()?;
        self.prg_bank_b = reader.read_u8()?;
        self.irq_counter = reader.read_u16()?;
        self.irq_enabled = reader.read_bool()?;
        self.irq_clock = reader.read_bool()?;
        self.irq_pending = reader.read_bool()?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        reader.read_bytes_into(&mut self.prg_ram)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM presence mismatch for FME-7 save state",
                ));
            }
        }
        self.audio.borrow_mut().load_state(reader)?;
        Ok(())
    }
}

pub(super) fn new_fme7(
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mirroring: Mirroring,
) -> (Fme7, Vec<Box<dyn ExpansionAudioChip>>) {
    let audio = Rc::new(RefCell::new(Sunsoft5bAudio::new()));
    let chip = Sunsoft5bAudioChip::new(audio.clone());
    (
        Fme7::new(prg_rom, chr_rom, mirroring, audio),
        vec![Box::new(chip)],
    )
}
