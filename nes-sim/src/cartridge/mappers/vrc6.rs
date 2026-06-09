use std::cell::RefCell;
use std::rc::Rc;

use super::Mapper;
use crate::apu::ExpansionAudioChip;
use crate::cartridge::Mirroring;
use crate::cartridge::expansion_audio::vrc6::{Vrc6Audio, Vrc6AudioChip};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const IRQ_PRESCALER_PERIOD: i32 = 341;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

pub(super) struct Vrc6 {
    prg_rom: Vec<u8>,
    chr_memory: ChrMemory,
    prg_bank_0: u8,
    prg_bank_1: u8,
    chr_banks: [u8; 8],
    mirroring: Mirroring,
    irq_latch: u8,
    irq_counter: u8,
    irq_prescaler: i32,
    irq_enabled: bool,
    irq_mode: bool,
    irq_pending: bool,
    irq_ack: bool,
    mapper_id: u16,
    audio: Rc<RefCell<Vrc6Audio>>,
}

impl Vrc6 {
    pub(super) fn new(
        prg_rom: Vec<u8>,
        chr_data: Vec<u8>,
        mirroring: Mirroring,
        mapper_id: u16,
        audio: Rc<RefCell<Vrc6Audio>>,
    ) -> Self {
        let chr_memory = if chr_data.is_empty() {
            ChrMemory::Ram(vec![0; 0x2000])
        } else {
            ChrMemory::Rom(chr_data)
        };

        Self {
            prg_rom,
            chr_memory,
            prg_bank_0: 0,
            prg_bank_1: 1,
            chr_banks: [0, 1, 2, 3, 4, 5, 6, 7],
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_prescaler: IRQ_PRESCALER_PERIOD,
            irq_enabled: false,
            irq_mode: false,
            irq_pending: false,
            irq_ack: false,
            mapper_id,
            audio,
        }
    }

    fn register_bits(&self, addr: u16) -> (bool, bool) {
        match self.mapper_id {
            24 => (addr & 0x01 != 0, addr & 0x02 != 0),
            26 => (addr & 0x02 != 0, addr & 0x01 != 0),
            _ => unreachable!(),
        }
    }

    fn tick_irq(&mut self) {
        if !self.irq_enabled {
            return;
        }
        if self.irq_mode {
            self.scanline_count();
        } else {
            self.irq_prescaler -= 3;
            if self.irq_prescaler <= 0 {
                self.irq_prescaler += IRQ_PRESCALER_PERIOD;
                self.scanline_count();
            }
        }
    }

    fn scanline_count(&mut self) {
        if self.irq_counter == 0xFF {
            self.irq_counter = self.irq_latch;
            self.irq_pending = true;
        } else {
            self.irq_counter = self.irq_counter.wrapping_add(1);
        }
    }
}

impl Mapper for Vrc6 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.prg_bank_0 as usize;
                let offset = (addr - 0x8000) as usize + bank * PRG_BANK_16K;
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xC000..=0xDFFF => {
                let bank = self.prg_bank_1 as usize;
                let offset = (addr - 0xC000) as usize + bank * PRG_BANK_8K;
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            0xE000..=0xFFFF => {
                let last_8k = self.prg_rom.len().saturating_sub(PRG_BANK_8K);
                let offset = (addr - 0xE000) as usize + last_8k;
                Some(self.prg_rom[offset % self.prg_rom.len()])
            }
            _ => None,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) -> bool {
        match addr {
            0x8000..=0x8FFF => {
                self.prg_bank_0 = data;
                true
            }
            0xC000..=0xCFFF => {
                self.prg_bank_1 = data;
                true
            }
            0xB000..=0xBFFF => {
                let (bit0, bit1) = self.register_bits(addr);
                if bit0 && bit1 {
                    self.mirroring = match (data >> 2) & 0x03 {
                        0 => Mirroring::Vertical,
                        1 => Mirroring::Horizontal,
                        2 => Mirroring::SPAGE0,
                        3 => Mirroring::SPAGE1,
                        _ => unreachable!(),
                    };
                } else {
                    let mut audio = self.audio.borrow_mut();
                    match (bit1, bit0) {
                        (false, false) => audio.saw.write_reg0(data),
                        (false, true) => audio.saw.write_reg1(data),
                        (true, false) => audio.saw.write_reg2(data),
                        _ => {}
                    }
                }
                true
            }
            0x9000..=0x9FFF | 0xA000..=0xAFFF => {
                let (bit0, bit1) = self.register_bits(addr);
                let pulse_idx = if addr & 0xF000 == 0x9000 { 0 } else { 1 };
                let mut audio = self.audio.borrow_mut();
                let pulse = if pulse_idx == 0 {
                    &mut audio.pulse1
                } else {
                    &mut audio.pulse2
                };
                match (bit1, bit0) {
                    (false, false) => pulse.write_reg0(data),
                    (false, true) => pulse.write_reg1(data),
                    (true, false) => pulse.write_reg2(data),
                    _ => {}
                }
                true
            }
            0xD000..=0xDFFF => {
                let (bit0, bit1) = self.register_bits(addr);
                let index = if bit1 { 2 } else { 0 } + if bit0 { 1 } else { 0 };
                self.chr_banks[index] = data;
                true
            }
            0xE000..=0xEFFF => {
                let (bit0, bit1) = self.register_bits(addr);
                let index = if bit1 { 2 } else { 0 } + if bit0 { 1 } else { 0 } + 4;
                self.chr_banks[index] = data;
                true
            }
            0xF000..=0xFFFF => {
                let (bit0, bit1) = self.register_bits(addr);
                match (bit1, bit0) {
                    (false, false) => {
                        self.irq_latch = data;
                    }
                    (false, true) => {
                        self.irq_ack = (data & 0x01) != 0;
                        self.irq_enabled = (data & 0x02) != 0;
                        self.irq_mode = (data & 0x04) != 0;
                        if self.irq_enabled {
                            self.irq_counter = self.irq_latch;
                            self.irq_prescaler = IRQ_PRESCALER_PERIOD;
                        }
                        self.irq_pending = false;
                    }
                    (true, false) => {
                        self.irq_enabled = self.irq_ack;
                        self.irq_pending = false;
                    }
                    _ => {}
                }
                true
            }
            _ => false,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        let addr = addr as usize & 0x1FFF;
        match &self.chr_memory {
            ChrMemory::Rom(rom) => {
                let bank = self.chr_banks[addr / CHR_BANK_1K] as usize;
                let offset = (addr % CHR_BANK_1K) + bank * CHR_BANK_1K;
                Some(rom[offset % rom.len()])
            }
            ChrMemory::Ram(ram) => Some(ram[addr]),
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) -> bool {
        if let ChrMemory::Ram(ram) = &mut self.chr_memory {
            ram[addr as usize & 0x1FFF] = data;
            return true;
        }
        false
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_line(&self) -> bool {
        self.irq_pending
    }

    fn tick_cpu_cycle(&mut self) {
        self.tick_irq();
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.prg_bank_0);
        writer.write_u8(self.prg_bank_1);
        for &bank in &self.chr_banks {
            writer.write_u8(bank);
        }
        writer.write_u8(self.mirroring as u8);
        writer.write_u8(self.irq_latch);
        writer.write_u8(self.irq_counter);
        writer.write_i16(self.irq_prescaler as i16);
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.irq_mode);
        writer.write_bool(self.irq_pending);
        writer.write_bool(self.irq_ack);
        self.audio.borrow().save_state(writer);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.prg_bank_0 = reader.read_u8()?;
        self.prg_bank_1 = reader.read_u8()?;
        for bank in &mut self.chr_banks {
            *bank = reader.read_u8()?;
        }
        self.mirroring = match reader.read_u8()? {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::FourScreen,
            3 => Mirroring::SPAGE0,
            4 => Mirroring::SPAGE1,
            _ => {
                return Err(SaveStateError::InvalidData(
                    "invalid mirroring value in VRC6 state",
                ));
            }
        };
        self.irq_latch = reader.read_u8()?;
        self.irq_counter = reader.read_u8()?;
        self.irq_prescaler = reader.read_i16()? as i32;
        self.irq_enabled = reader.read_bool()?;
        self.irq_mode = reader.read_bool()?;
        self.irq_pending = reader.read_bool()?;
        self.irq_ack = reader.read_bool()?;
        self.audio.borrow_mut().load_state(reader)?;
        Ok(())
    }
}

pub(super) fn new_vrc6(
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mirroring: Mirroring,
    mapper_id: u16,
) -> (Vrc6, Vec<Box<dyn ExpansionAudioChip>>) {
    let audio = Rc::new(RefCell::new(Vrc6Audio::new()));
    let chip = Vrc6AudioChip::new(audio.clone());
    (
        Vrc6::new(prg_rom, chr_rom, mirroring, mapper_id, audio),
        vec![Box::new(chip)],
    )
}
