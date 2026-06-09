use super::LENGTH_TABLE;

const TRIANGLE_TABLE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15,
];

#[derive(Clone, Copy, Default)]
pub(super) struct TriangleChannel {
    pub(super) enabled: bool,
    pub(super) length_counter: u8,
    pub(super) timer_reload: u16,
    pub(super) timer_counter: u16,
    pub(super) seq_step: u8,
    pub(super) control_flag: bool,
    pub(super) linear_reload_value: u8,
    pub(super) linear_counter: u8,
    pub(super) linear_reload_flag: bool,
}

impl TriangleChannel {
    pub(super) fn write_linear_control(&mut self, value: u8) {
        self.control_flag = (value & 0x80) != 0;
        self.linear_reload_value = value & 0x7F;
    }

    pub(super) fn write_timer_low(&mut self, value: u8) {
        self.timer_reload = (self.timer_reload & 0x0700) | value as u16;
    }

    pub(super) fn write_timer_high(&mut self, value: u8, length_enabled: bool) {
        self.timer_reload = (self.timer_reload & 0x00FF) | (((value & 0x07) as u16) << 8);
        if length_enabled {
            self.length_counter = LENGTH_TABLE[(value >> 3) as usize];
        }
        self.linear_reload_flag = true;
    }

    pub(super) fn tick_timer(&mut self) {
        if self.timer_reload < 2 {
            return;
        }
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_reload;
            if self.length_counter > 0 && self.linear_counter > 0 {
                self.seq_step = (self.seq_step + 1) & 0x1F;
            }
        } else {
            self.timer_counter -= 1;
        }
    }

    pub(super) fn quarter_frame_tick(&mut self) {
        if self.linear_reload_flag {
            self.linear_counter = self.linear_reload_value;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }

        if !self.control_flag {
            self.linear_reload_flag = false;
        }
    }

    pub(super) fn half_frame_tick(&mut self) {
        if !self.control_flag && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    pub(super) fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length_counter = 0;
        }
    }

    #[allow(dead_code)]
    pub(super) fn output(&self) -> u8 {
        if !self.enabled
            || self.timer_reload < 2
            || self.length_counter == 0
            || self.linear_counter == 0
        {
            return 0;
        }
        TRIANGLE_TABLE[self.seq_step as usize]
    }
}
