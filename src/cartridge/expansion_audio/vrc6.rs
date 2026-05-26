use std::cell::RefCell;
use std::rc::Rc;

use crate::apu::ExpansionAudioChip;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const PULSE_STEPS: u8 = 16;

pub(crate) struct Vrc6Pulse {
    pub enabled: bool,
    pub digitized: bool,
    pub duty: u8,
    pub volume: u8,
    pub wavelength: u16,
    pub timer: u16,
    pub step: u8,
}

impl Vrc6Pulse {
    pub(crate) fn new() -> Self {
        Self {
            enabled: false,
            digitized: false,
            duty: 1,
            volume: 0,
            wavelength: 1,
            timer: 1,
            step: 0,
        }
    }

    pub(crate) fn write_reg0(&mut self, data: u8) {
        self.volume = data & 0x0F;
        self.duty = ((data >> 4) & 0x07) + 1;
        self.digitized = (data & 0x80) != 0;
    }

    pub(crate) fn write_reg1(&mut self, data: u8) {
        self.wavelength = (self.wavelength & 0x0F00) | u16::from(data);
    }

    pub(crate) fn write_reg2(&mut self, data: u8) {
        self.wavelength = (self.wavelength & 0x00FF) | (u16::from(data & 0x0F) << 8);
        self.enabled = (data & 0x80) != 0;
    }

    fn active(&self) -> bool {
        self.enabled && (self.digitized || self.volume > 0) && self.wavelength >= 4
    }

    pub(crate) fn tick(&mut self) {
        if !self.active() {
            return;
        }
        self.timer = self.timer.saturating_sub(1);
        if self.timer == 0 {
            self.timer = self.wavelength + 1;
            self.step = (self.step + 1) % PULSE_STEPS;
        }
    }

    pub(crate) fn output(&self) -> f32 {
        if !self.active() {
            return 0.0;
        }
        if self.digitized || self.step < self.duty {
            f32::from(self.volume) / 15.0 * 0.3
        } else {
            0.0
        }
    }

    #[allow(dead_code)]
    pub(crate) fn reset(&mut self) {
        self.enabled = false;
        self.digitized = false;
        self.duty = 1;
        self.volume = 0;
        self.wavelength = 1;
        self.timer = 1;
        self.step = 0;
    }

    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.enabled);
        writer.write_bool(self.digitized);
        writer.write_u8(self.duty);
        writer.write_u8(self.volume);
        writer.write_u16(self.wavelength);
        writer.write_u16(self.timer);
        writer.write_u8(self.step);
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        self.enabled = reader.read_bool()?;
        self.digitized = reader.read_bool()?;
        self.duty = reader.read_u8()?;
        self.volume = reader.read_u8()?;
        self.wavelength = reader.read_u16()?;
        self.timer = reader.read_u16()?;
        self.step = reader.read_u8()?;
        Ok(())
    }
}

pub(crate) struct Vrc6Saw {
    pub enabled: bool,
    pub phase: u8,
    pub wavelength: u16,
    pub timer: u16,
    pub step: u8,
    pub amp: u8,
}

impl Vrc6Saw {
    pub(crate) fn new() -> Self {
        Self {
            enabled: false,
            phase: 0,
            wavelength: 1,
            timer: 1,
            step: 0,
            amp: 0,
        }
    }

    pub(crate) fn write_reg0(&mut self, data: u8) {
        self.phase = data & 0x3F;
    }

    pub(crate) fn write_reg1(&mut self, data: u8) {
        self.wavelength = (self.wavelength & 0x0F00) | u16::from(data);
    }

    pub(crate) fn write_reg2(&mut self, data: u8) {
        self.wavelength = (self.wavelength & 0x00FF) | (u16::from(data & 0x0F) << 8);
        self.enabled = (data & 0x80) != 0;
    }

    fn active(&self) -> bool {
        self.enabled && self.phase > 0 && self.wavelength >= 4
    }

    pub(crate) fn tick(&mut self) {
        if !self.active() {
            return;
        }
        self.timer = self.timer.saturating_sub(1);
        if self.timer == 0 {
            self.timer = (self.wavelength + 1) * 2;
            self.amp = self.amp.wrapping_add(self.phase);
            self.step += 1;
            if self.step >= 7 {
                self.step = 0;
                self.amp = 0;
            }
        }
    }

    pub(crate) fn output(&self) -> f32 {
        if !self.active() {
            return 0.0;
        }
        (self.amp >> 3) as f32 / 31.0 * 0.3
    }

    #[allow(dead_code)]
    pub(crate) fn reset(&mut self) {
        self.enabled = false;
        self.phase = 0;
        self.wavelength = 1;
        self.timer = 1;
        self.step = 0;
        self.amp = 0;
    }

    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.enabled);
        writer.write_u8(self.phase);
        writer.write_u16(self.wavelength);
        writer.write_u16(self.timer);
        writer.write_u8(self.step);
        writer.write_u8(self.amp);
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        self.enabled = reader.read_bool()?;
        self.phase = reader.read_u8()?;
        self.wavelength = reader.read_u16()?;
        self.timer = reader.read_u16()?;
        self.step = reader.read_u8()?;
        self.amp = reader.read_u8()?;
        Ok(())
    }
}

pub(crate) struct Vrc6Audio {
    pub pulse1: Vrc6Pulse,
    pub pulse2: Vrc6Pulse,
    pub saw: Vrc6Saw,
}

impl Vrc6Audio {
    pub(crate) fn new() -> Self {
        Self {
            pulse1: Vrc6Pulse::new(),
            pulse2: Vrc6Pulse::new(),
            saw: Vrc6Saw::new(),
        }
    }

    pub(crate) fn tick(&mut self) {
        self.pulse1.tick();
        self.pulse2.tick();
        self.saw.tick();
    }

    pub(crate) fn output(&self) -> f32 {
        self.pulse1.output() + self.pulse2.output() + self.saw.output()
    }

    #[allow(dead_code)]
    pub(crate) fn reset(&mut self) {
        self.pulse1.reset();
        self.pulse2.reset();
        self.saw.reset();
    }

    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        self.pulse1.save_state(writer);
        self.pulse2.save_state(writer);
        self.saw.save_state(writer);
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        self.pulse1.load_state(reader)?;
        self.pulse2.load_state(reader)?;
        self.saw.load_state(reader)?;
        Ok(())
    }
}

pub(crate) struct Vrc6AudioChip {
    audio: Rc<RefCell<Vrc6Audio>>,
}

impl Vrc6AudioChip {
    pub(crate) fn new(audio: Rc<RefCell<Vrc6Audio>>) -> Self {
        Self { audio }
    }
}

impl ExpansionAudioChip for Vrc6AudioChip {
    fn tick_cpu_cycle(&mut self) {
        self.audio.borrow_mut().tick();
    }

    fn output_sample(&self) -> f32 {
        self.audio.borrow().output()
    }
}
