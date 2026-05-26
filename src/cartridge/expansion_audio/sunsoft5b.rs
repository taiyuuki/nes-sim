use std::cell::RefCell;
use std::rc::Rc;

use crate::apu::ExpansionAudioChip;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

const DUTY_STEPS: u8 = 32;
const DUTY_HIGH: u8 = 16;

const VOLUME_TABLE: [f32; 16] = [
    0.0, 0.007, 0.010, 0.014, 0.019, 0.026, 0.036, 0.049, 0.066, 0.089, 0.119, 0.158, 0.209, 0.275,
    0.360, 0.468,
];

struct SquareChannel {
    enabled: bool,
    period: u16,
    divider: u16,
    step: u8,
}

impl SquareChannel {
    fn new() -> Self {
        Self {
            enabled: true,
            period: 1,
            divider: 0,
            step: 0,
        }
    }

    fn set_period_lo(&mut self, data: u8) {
        self.period = (self.period & 0x0F00) | data as u16;
    }

    fn set_period_hi(&mut self, data: u8) {
        self.period = (self.period & 0x00FF) | (((data & 0x0F) as u16) << 8);
    }

    fn active(&self) -> bool {
        self.enabled && self.period >= 8
    }

    fn tick(&mut self) {
        if !self.active() {
            return;
        }
        self.divider += 1;
        while self.divider >= self.period {
            self.divider -= self.period;
            self.step = (self.step + 1) % DUTY_STEPS;
        }
    }

    fn output(&self) -> f32 {
        if !self.active() {
            return 0.0;
        }
        if self.step < DUTY_HIGH { 1.0 } else { 0.0 }
    }
}

pub(crate) struct Sunsoft5bAudio {
    channels: [SquareChannel; 3],
    volumes: [u8; 3],
    use_envelope: [bool; 3],
    reg_select: u8,
}

impl Sunsoft5bAudio {
    pub(crate) fn new() -> Self {
        Self {
            channels: [
                SquareChannel::new(),
                SquareChannel::new(),
                SquareChannel::new(),
            ],
            volumes: [0; 3],
            use_envelope: [false; 3],
            reg_select: 0,
        }
    }

    pub(crate) fn write_address(&mut self, data: u8) {
        self.reg_select = data & 0x0F;
    }

    pub(crate) fn write_data(&mut self, data: u8) {
        match self.reg_select {
            0x0 => self.channels[0].set_period_lo(data),
            0x1 => self.channels[0].set_period_hi(data),
            0x2 => self.channels[1].set_period_lo(data),
            0x3 => self.channels[1].set_period_hi(data),
            0x4 => self.channels[2].set_period_lo(data),
            0x5 => self.channels[2].set_period_hi(data),
            0x7 => {
                for i in 0..3 {
                    self.channels[i].enabled = (data & (1 << i)) == 0;
                }
            }
            0x8 => {
                self.volumes[0] = data & 0x0F;
                self.use_envelope[0] = (data & 0x10) != 0;
            }
            0x9 => {
                self.volumes[1] = data & 0x0F;
                self.use_envelope[1] = (data & 0x10) != 0;
            }
            0xA => {
                self.volumes[2] = data & 0x0F;
                self.use_envelope[2] = (data & 0x10) != 0;
            }
            _ => {}
        }
    }

    pub(crate) fn tick(&mut self) {
        for ch in &mut self.channels {
            ch.tick();
        }
    }

    pub(crate) fn output(&self) -> f32 {
        let mut out = 0.0;
        for i in 0..3 {
            if self.use_envelope[i] {
                continue;
            }
            out += VOLUME_TABLE[self.volumes[i] as usize] * self.channels[i].output();
        }
        out
    }

    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.reg_select);
        for ch in &self.channels {
            writer.write_bool(ch.enabled);
            writer.write_u16(ch.period);
            writer.write_u16(ch.divider);
            writer.write_u8(ch.step);
        }
        for &vol in &self.volumes {
            writer.write_u8(vol);
        }
        for &env in &self.use_envelope {
            writer.write_bool(env);
        }
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        self.reg_select = reader.read_u8()?;
        for ch in &mut self.channels {
            ch.enabled = reader.read_bool()?;
            ch.period = reader.read_u16()?;
            ch.divider = reader.read_u16()?;
            ch.step = reader.read_u8()?;
        }
        for vol in &mut self.volumes {
            *vol = reader.read_u8()?;
        }
        for env in &mut self.use_envelope {
            *env = reader.read_bool()?;
        }
        Ok(())
    }
}

pub(crate) struct Sunsoft5bAudioChip {
    audio: Rc<RefCell<Sunsoft5bAudio>>,
}

impl Sunsoft5bAudioChip {
    pub(crate) fn new(audio: Rc<RefCell<Sunsoft5bAudio>>) -> Self {
        Self { audio }
    }
}

impl ExpansionAudioChip for Sunsoft5bAudioChip {
    fn tick_cpu_cycle(&mut self) {
        self.audio.borrow_mut().tick();
    }

    fn output_sample(&self) -> f32 {
        self.audio.borrow().output()
    }
}
