use super::LENGTH_TABLE;

#[derive(Clone, Copy)]
pub(super) struct NoiseChannel {
    pub(super) enabled: bool,
    pub(super) length_counter: u8,
    pub(super) mode: bool,
    pub(super) timer_reload: u16,
    pub(super) timer_counter: u16,
    pub(super) length_halt: bool,
    pub(super) constant_volume: bool,
    pub(super) envelope_period: u8,
    pub(super) envelope_start: bool,
    pub(super) envelope_divider: u8,
    pub(super) envelope_decay: u8,
    pub(super) shift_register: u16,
}

impl Default for NoiseChannel {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: false,
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
        }
    }
}

impl NoiseChannel {
    pub(super) fn write_control(&mut self, value: u8) {
        self.length_halt = (value & 0x20) != 0;
        self.constant_volume = (value & 0x10) != 0;
        self.envelope_period = value & 0x0F;
    }

    pub(super) fn write_period(&mut self, value: u8) {
        const NOISE_PERIODS: [u16; 16] = [
            4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
        ];
        self.mode = (value & 0x80) != 0;
        self.timer_reload = NOISE_PERIODS[(value & 0x0F) as usize];
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
