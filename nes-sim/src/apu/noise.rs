use super::LENGTH_TABLE;
use crate::cartridge::TVSystem;

// PAL RP2A07 的LFSR分频链与NTSC不同（Nesdev APU Noise）；
// Dendy 克隆芯片同为 PAL 型（Mesen 同款映射）
const NOISE_PERIODS_NTSC: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];
const NOISE_PERIODS_PAL: [u16; 16] = [
    4, 8, 14, 30, 60, 88, 118, 148, 188, 236, 354, 472, 708, 944, 1890, 3778,
];

#[derive(Clone, Copy)]
pub(super) struct NoiseChannel {
    pub(super) enabled: bool,
    pub(super) length_counter: u8,
    pub(super) mode: bool,
    pub(super) rate_index: u8,
    pub(super) timer_reload: u16,
    pub(super) timer_counter: u16,
    pub(super) length_halt: bool,
    pub(super) constant_volume: bool,
    pub(super) envelope_period: u8,
    pub(super) envelope_start: bool,
    pub(super) envelope_divider: u8,
    pub(super) envelope_decay: u8,
    pub(super) shift_register: u16,
    noise_periods: &'static [u16; 16],
}

impl Default for NoiseChannel {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: false,
            rate_index: 0,
            timer_reload: 4,
            timer_counter: 0,
            length_counter: 0,
            length_halt: false,
            constant_volume: false,
            envelope_period: 0,
            envelope_start: false,
            envelope_divider: 0,
            envelope_decay: 0,
            shift_register: 1,
            noise_periods: &NOISE_PERIODS_NTSC,
        }
    }
}

impl NoiseChannel {
    pub(super) fn set_tv_system(&mut self, tv: TVSystem) {
        self.noise_periods = match tv {
            TVSystem::NTSC => &NOISE_PERIODS_NTSC,
            TVSystem::PAL | TVSystem::DENDY => &NOISE_PERIODS_PAL,
        };
        self.timer_reload = self.noise_periods[self.rate_index as usize];
    }

    pub(super) fn write_control(&mut self, value: u8) {
        self.length_halt = (value & 0x20) != 0;
        self.constant_volume = (value & 0x10) != 0;
        self.envelope_period = value & 0x0F;
    }

    pub(super) fn write_period(&mut self, value: u8) {
        self.mode = (value & 0x80) != 0;
        self.rate_index = value & 0x0F;
        self.timer_reload = self.noise_periods[self.rate_index as usize];
    }

    pub(super) fn write_length(&mut self, value: u8, length_enabled: bool) {
        if length_enabled {
            self.length_counter = LENGTH_TABLE[(value >> 3) as usize];
        }
        self.envelope_start = true;
    }

    pub(super) fn tick_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_reload;
            let tap_bit = if self.mode { 6 } else { 1 };
            let feedback = (self.shift_register & 0x01) ^ ((self.shift_register >> tap_bit) & 0x01);
            self.shift_register >>= 1;
            self.shift_register |= feedback << 14;
        } else {
            self.timer_counter -= 1;
        }
    }

    pub(super) fn quarter_frame_tick(&mut self) {
        if self.envelope_start {
            self.envelope_start = false;
            self.envelope_decay = 15;
            self.envelope_divider = self.envelope_period;
            return;
        }

        if self.envelope_divider == 0 {
            self.envelope_divider = self.envelope_period;
            if self.envelope_decay > 0 {
                self.envelope_decay -= 1;
            } else if self.length_halt {
                self.envelope_decay = 15;
            }
        } else {
            self.envelope_divider -= 1;
        }
    }

    pub(super) fn half_frame_tick(&mut self) {
        if !self.length_halt && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    pub(super) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length_counter = 0;
        }
    }

    fn envelope_volume(&self) -> u8 {
        if self.constant_volume {
            self.envelope_period
        } else {
            self.envelope_decay
        }
    }

    pub(super) fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 || (self.shift_register & 0x01) != 0 {
            return 0;
        }
        self.envelope_volume()
    }
}
