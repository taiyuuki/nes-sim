use std::cell::RefCell;
use std::rc::Rc;

use crate::apu::ExpansionAudioChip;

const DUTY_TABLE: [[bool; 8]; 4] = [
    [false, false, false, false, false, false, false, true], // 12.5% -> FCEUX says 87.5%
    [false, false, false, false, false, false, true, true],  // 25% -> 75%
    [false, false, false, false, false, true, true, true],   // 50%
    [false, false, false, false, true, true, true, true],    // 75% -> 25%
];

pub(crate) struct Mmc5Pulse {
    enabled: bool,
    duty: u8,
    constant_volume: bool,
    envelope_period: u8,
    envelope_start: bool,
    #[allow(dead_code)]
    envelope_divider: u8,
    envelope_decay: u8,
    timer_reload: u16,
    timer_counter: u16,
    seq_step: u8,
}

impl Mmc5Pulse {
    fn new() -> Self {
        Self {
            enabled: false,
            duty: 0,
            constant_volume: false,
            envelope_period: 0,
            envelope_start: false,
            envelope_divider: 0,
            envelope_decay: 0,
            timer_reload: 0,
            timer_counter: 0,
            seq_step: 0,
        }
    }

    fn write_control(&mut self, value: u8) {
        self.duty = (value >> 6) & 0x03;
        self.constant_volume = (value & 0x10) != 0;
        self.envelope_period = value & 0x0F;
    }

    fn write_timer_low(&mut self, value: u8) {
        self.timer_reload = (self.timer_reload & 0x0700) | value as u16;
    }

    fn write_timer_high(&mut self, value: u8) {
        self.timer_reload = (self.timer_reload & 0x00FF) | (((value & 0x07) as u16) << 8);
        self.seq_step = 0;
        self.timer_counter = self.timer_reload;
        self.envelope_start = true;
    }

    fn active(&self) -> bool {
        self.enabled && self.timer_reload >= 8
    }

    fn tick(&mut self) {
        if !self.active() {
            return;
        }
        if self.timer_counter > 0 {
            self.timer_counter -= 1;
        } else {
            self.timer_counter = self.timer_reload;
            self.seq_step = (self.seq_step + 1) & 7;
        }
    }

    fn output(&self) -> u8 {
        if !self.active() {
            return 0;
        }
        if !DUTY_TABLE[self.duty as usize][self.seq_step as usize] {
            return 0;
        }
        if self.constant_volume {
            self.envelope_period
        } else {
            self.envelope_decay
        }
    }

    #[allow(dead_code)]
    fn clock_envelope(&mut self) {
        if self.envelope_start {
            self.envelope_start = false;
            self.envelope_decay = 15;
            self.envelope_divider = self.envelope_period;
            return;
        }
        if self.envelope_divider > 0 {
            self.envelope_divider -= 1;
        } else {
            self.envelope_divider = self.envelope_period;
            if self.envelope_decay > 0 {
                self.envelope_decay -= 1;
            }
        }
    }
}

pub(crate) struct Mmc5Audio {
    pulse1: Mmc5Pulse,
    pulse2: Mmc5Pulse,
    enable: u8,
    pcm_control: u8,
    pcm_value: u8,
}

impl Mmc5Audio {
    pub(crate) fn new() -> Self {
        Self {
            pulse1: Mmc5Pulse::new(),
            pulse2: Mmc5Pulse::new(),
            enable: 0,
            pcm_control: 0,
            pcm_value: 0,
        }
    }

    pub(crate) fn write(&mut self, addr: u16, data: u8) {
        match addr {
            0x5000 => self.pulse1.write_control(data),
            0x5002 => self.pulse1.write_timer_low(data),
            0x5003 => self.pulse1.write_timer_high(data),
            0x5004 => self.pulse2.write_control(data),
            0x5006 => self.pulse2.write_timer_low(data),
            0x5007 => self.pulse2.write_timer_high(data),
            0x5010 => self.pcm_control = data,
            0x5011 => self.pcm_value = data & 0x7F,
            0x5015 => {
                self.enable = data & 0x03;
                if (data & 0x01) == 0 {
                    self.pulse1.enabled = false;
                }
                if (data & 0x02) == 0 {
                    self.pulse2.enabled = false;
                }
                if (data & 0x01) != 0 {
                    self.pulse1.enabled = true;
                }
                if (data & 0x02) != 0 {
                    self.pulse2.enabled = true;
                }
            }
            _ => {}
        }
    }

    pub(crate) fn status(&self) -> u8 {
        let mut s = 0u8;
        if self.pulse1.enabled {
            s |= 0x01;
        }
        if self.pulse2.enabled {
            s |= 0x02;
        }
        s
    }

    pub(crate) fn tick(&mut self) {
        self.pulse1.tick();
        self.pulse2.tick();
    }

    #[allow(dead_code)]
    fn clock_envelopes(&mut self) {
        self.pulse1.clock_envelope();
        self.pulse2.clock_envelope();
    }

    pub(crate) fn output(&self) -> f32 {
        let p1 = self.pulse1.output() as f32;
        let p2 = self.pulse2.output() as f32;
        let pcm = if (self.pcm_control & 0x40) == 0 && self.pcm_value != 0 {
            self.pcm_value as f32
        } else {
            0.0
        };
        // MMC5 pulse mixing: similar to APU pulse mix formula
        let pulse = p1 + p2;
        let pulse_mix = if pulse > 0.0 {
            95.88 / (8128.0 / pulse + 100.0)
        } else {
            0.0
        };
        pulse_mix + pcm / 127.0 * 0.2
    }
}

pub(crate) struct Mmc5AudioChip {
    audio: Rc<RefCell<Mmc5Audio>>,
}

impl Mmc5AudioChip {
    pub(crate) fn new(audio: Rc<RefCell<Mmc5Audio>>) -> Self {
        Self { audio }
    }
}

impl ExpansionAudioChip for Mmc5AudioChip {
    fn cpu_write(&mut self, addr: u16, data: u8) {
        if (0x5000..=0x5015).contains(&addr) {
            self.audio.borrow_mut().write(addr, data);
        }
    }

    fn cpu_read(&mut self, addr: u16) -> Option<u8> {
        if addr == 0x5015 {
            Some(self.audio.borrow().status())
        } else {
            None
        }
    }

    fn tick_cpu_cycle(&mut self) {
        self.audio.borrow_mut().tick();
    }

    fn clock_quarter_frame(&mut self) {
        self.audio.borrow_mut().clock_envelopes();
    }

    fn output_sample(&self) -> f32 {
        self.audio.borrow().output()
    }
}
