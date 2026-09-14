use std::cell::RefCell;
use std::rc::Rc;

use super::Mapper;
use crate::apu::ExpansionAudioChip;
use crate::cartridge::Mirroring;
use crate::cartridge::expansion_audio::vrc7::{Vrc7Audio, Vrc7AudioChip};
use crate::cartridge::mappers::{decode_mirroring, encode_mirroring};
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PRG_RAM_LEN: usize = 0x2000;
const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const IRQ_PRESCALER_PERIOD: i32 = 341;

enum ChrMemory {
    Rom(Vec<u8>),
    Ram(Vec<u8>),
}

/// Konami VRC7 board (iNES mapper 085, Lagrange Point / Tiny Toon Adventures
/// 2). Adds a YM2413-compatible FM synthesizer whose registers are exposed at
/// $9010/$9030.
pub(super) struct Vrc7 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: ChrMemory,
    prg_banks: [u8; 3],
    chr_banks: [u8; 8],
    mirroring: Mirroring,
    irq_latch: u8,
    irq_counter: u8,
    irq_prescaler: i32,
    irq_enabled: bool,
    irq_ack_enable: bool,
    irq_cpu_mode: bool,
    irq_pending: bool,
    audio_reg: u8,
    audio: Rc<RefCell<Vrc7Audio>>,
}

impl Vrc7 {
    pub(super) fn new(
        prg_rom: Vec<u8>,
        chr_rom: Vec<u8>,
        mirroring: Mirroring,
        audio: Rc<RefCell<Vrc7Audio>>,
    ) -> Self {
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
            chr_banks: [0; 8],
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_prescaler: IRQ_PRESCALER_PERIOD,
            irq_enabled: false,
            irq_ack_enable: false,
            irq_cpu_mode: false,
            irq_pending: false,
            audio_reg: 0,
            audio,
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

    fn scanline_count(&mut self) {
        if self.irq_counter == 0xFF {
            self.irq_counter = self.irq_latch;
            self.irq_pending = true;
        } else {
            self.irq_counter += 1;
        }
    }
}

impl Mapper for Vrc7 {
    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        match addr {
            0x6000..=0x7FFF => Some(self.prg_ram[(addr - 0x6000) as usize]),
            0x8000..=0xDFFF => {
                let slot = ((addr - 0x8000) as usize) / PRG_BANK_8K;
                let bank = (self.prg_banks[slot] as usize) % self.prg_bank_count();
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
        if !matches!(addr, 0x6000..=0xFFFF) {
            return false;
        }
        if matches!(addr, 0x6000..=0x7FFF) {
            self.prg_ram[(addr - 0x6000) as usize] = data;
            return true;
        }

        // The chip folds address bit 3 into bit 4, merging the two register
        // windows into one decoding space.
        let addr = addr | ((addr & 0x0008) << 1);
        match addr {
            0xA000..=0xDFFF => {
                let index = (((addr >> 4) & 1) | ((addr - 0xA000) >> 11)) as usize;
                self.chr_banks[index] = data;
            }
            0x9030 => {
                self.audio.borrow_mut().write(self.audio_reg, data);
            }
            _ => match addr & 0xF010 {
                0x8000 => self.prg_banks[0] = data,
                0x8010 => self.prg_banks[1] = data,
                0x9000 => self.prg_banks[2] = data,
                0x9010 => self.audio_reg = data,
                0xE000 => {
                    self.mirroring = match data & 0x03 {
                        0 => Mirroring::Vertical,
                        1 => Mirroring::Horizontal,
                        2 => Mirroring::SPAGE0,
                        _ => Mirroring::SPAGE1,
                    };
                }
                0xE010 => {
                    self.irq_latch = data;
                    self.irq_pending = false;
                }
                0xF000 => {
                    self.irq_cpu_mode = (data & 0x04) != 0;
                    self.irq_ack_enable = (data & 0x01) != 0;
                    self.irq_enabled = (data & 0x02) != 0;
                    if self.irq_enabled {
                        self.irq_counter = self.irq_latch;
                        self.irq_prescaler = IRQ_PRESCALER_PERIOD;
                    }
                    self.irq_pending = false;
                }
                0xF010 => {
                    self.irq_enabled = self.irq_ack_enable;
                    self.irq_pending = false;
                }
                _ => {}
            },
        }
        true
    }

    fn ppu_read(&mut self, addr: u16) -> Option<u8> {
        if !matches!(addr, 0x0000..=0x1FFF) {
            return None;
        }
        let bank =
            (self.chr_banks[addr as usize / CHR_BANK_1K] as usize) % self.chr_bank_count_1k();
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
        let bank =
            (self.chr_banks[addr as usize / CHR_BANK_1K] as usize) % self.chr_bank_count_1k();
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
        self.irq_pending
    }

    fn tick_cpu_cycle(&mut self) {
        if !self.irq_enabled {
            return;
        }
        if self.irq_cpu_mode {
            self.scanline_count();
        } else {
            self.irq_prescaler -= 3;
            if self.irq_prescaler <= 0 {
                self.irq_prescaler += IRQ_PRESCALER_PERIOD;
                self.scanline_count();
            }
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
        writer.write_u8(self.irq_latch);
        writer.write_u8(self.irq_counter);
        writer.write_i16(self.irq_prescaler as i16);
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.irq_ack_enable);
        writer.write_bool(self.irq_cpu_mode);
        writer.write_bool(self.irq_pending);
        writer.write_u8(self.audio_reg);
        self.audio.borrow().save_state(writer);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.prg_ram)?;
        let has_chr_ram = reader.read_bool()?;
        match (&mut self.chr, has_chr_ram) {
            (ChrMemory::Ram(chr_ram), true) => reader.read_bytes_into(chr_ram)?,
            (ChrMemory::Rom(_), false) => {}
            _ => {
                return Err(SaveStateError::InvalidData(
                    "CHR RAM mismatch for VRC7 save state",
                ));
            }
        }
        reader.read_bytes_into(&mut self.prg_banks)?;
        reader.read_bytes_into(&mut self.chr_banks)?;
        self.mirroring = decode_mirroring(reader.read_u8()?)?;
        self.irq_latch = reader.read_u8()?;
        self.irq_counter = reader.read_u8()?;
        self.irq_prescaler = reader.read_i16()? as i32;
        self.irq_enabled = reader.read_bool()?;
        self.irq_ack_enable = reader.read_bool()?;
        self.irq_cpu_mode = reader.read_bool()?;
        self.irq_pending = reader.read_bool()?;
        self.audio_reg = reader.read_u8()?;
        self.audio.borrow_mut().load_state(reader)?;
        Ok(())
    }
}

pub(super) fn new_vrc7(
    prg_rom: Vec<u8>,
    chr_rom: Vec<u8>,
    mirroring: Mirroring,
) -> (Vrc7, Vec<Box<dyn ExpansionAudioChip>>) {
    let audio = Rc::new(RefCell::new(Vrc7Audio::new()));
    let chip = Vrc7AudioChip::new(audio.clone());
    (
        Vrc7::new(prg_rom, chr_rom, mirroring, audio),
        vec![Box::new(chip)],
    )
}
