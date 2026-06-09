use super::LENGTH_TABLE;

#[derive(Clone, Copy, Default)]
pub(super) struct PulseChannel {
    pub(super) enabled: bool,
    pub(super) length_counter: u8,
    pub(super) duty: u8,
    pub(super) seq_step: u8,
    pub(super) timer_reload: u16,
    pub(super) timer_counter: u16,
    pub(super) length_halt: bool,
    pub(super) constant_volume: bool,
    pub(super) envelope_period: u8,
    pub(super) envelope_start: bool,
    pub(super) envelope_divider: u8,
    pub(super) envelope_decay: u8,
    pub(super) sweep_enabled: bool,
    pub(super) sweep_period: u8,
    pub(super) sweep_negate: bool,
    pub(super) sweep_shift: u8,
    pub(super) sweep_reload: bool,
    pub(super) sweep_divider: u8,
    pub(super) sweep_mute: bool,
    pub(super) sweep_negate_extra: u16,
}

impl PulseChannel {
    pub(super) fn new(is_pulse1: bool) -> Self {
        Self {
            sweep_negate_extra: if is_pulse1 { 0xFFFF } else { 0 },
            ..Self::default()
        }
    }

    pub(super) fn write_control(&mut self, value: u8) {
        self.duty = (value >> 6) & 0x03;
        self.length_halt = (value & 0x20) != 0;
        self.constant_volume = (value & 0x10) != 0;
        self.envelope_period = value & 0x0F;
    }

    pub(super) fn write_timer_low(&mut self, value: u8) {
        self.timer_reload = (self.timer_reload & 0x0700) | value as u16;
        self.refresh_sweep_mute();
    }

    pub(super) fn write_timer_high(&mut self, value: u8, length_enabled: bool) {
        self.timer_reload = (self.timer_reload & 0x00FF) | (((value & 0x07) as u16) << 8);
        self.seq_step = 0;
        self.timer_counter = self.timer_reload;
        self.envelope_start = true;
        if length_enabled {
            self.length_counter = LENGTH_TABLE[(value >> 3) as usize];
        }
        self.refresh_sweep_mute();
    }

    pub(super) fn write_sweep(&mut self, value: u8) {
        self.sweep_enabled = (value & 0x80) != 0;
        self.sweep_period = (value >> 4) & 0x07;
        self.sweep_negate = (value & 0x08) != 0;
        self.sweep_shift = value & 0x07;
        self.sweep_reload = true;
        self.refresh_sweep_mute();
    }

    pub(super) fn tick_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_reload;
            self.seq_step = (self.seq_step + 1) & 0x07;
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
        self.tick_sweep();
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
        if !self.enabled || self.length_counter == 0 || self.sweep_mute {
            return 0;
        }
        let high = match self.duty & 0x03 {
            0 => self.seq_step == 1,
            1 => self.seq_step == 1 || self.seq_step == 2,
            2 => self.seq_step >= 1 && self.seq_step <= 4,
            _ => self.seq_step == 0 || self.seq_step >= 3,
        };
        if high { self.envelope_volume() } else { 0 }
    }

    pub(super) fn target_period(&self) -> u16 {
        let change = self.timer_reload >> self.sweep_shift;
        if self.sweep_negate {
            self.timer_reload
                .wrapping_sub(change)
                .wrapping_add(self.sweep_negate_extra)
        } else {
            self.timer_reload.wrapping_add(change)
        }
    }

    pub(super) fn refresh_sweep_mute(&mut self) {
        let target = if self.sweep_shift == 0 {
            self.timer_reload
        } else {
            self.target_period()
        };
        self.sweep_mute = self.timer_reload < 8 || target > 0x07FF;
    }

    fn tick_sweep(&mut self) {
        if self.sweep_divider == 0
            && self.sweep_enabled
            && self.sweep_shift != 0
            && !self.sweep_mute
        {
            self.timer_reload = self.target_period();
        }

        if self.sweep_divider == 0 || self.sweep_reload {
            self.sweep_divider = self.sweep_period;
            self.sweep_reload = false;
        } else {
            self.sweep_divider -= 1;
        }
        self.refresh_sweep_mute();
    }
}
