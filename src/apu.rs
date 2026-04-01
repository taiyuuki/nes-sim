use crate::savestate::{SaveStateError, StateReader, StateWriter};

pub struct APU {
    cpu_cycle: u64,
    frame_counter_cycle: u32,
    frame_counter_reset_delay: Option<u8>,
    frame_irq_enabled: bool,
    frame_irq_flag: bool,
    frame_irq_line_low: bool,
    frame_irq_line_delay: u8,
    frame_counter_mode_five_step: bool,
    frame_irq_event_fired: bool,
    frame_irq_assert_window: u8,
    frame_irq_clear_after_cycle: Option<u64>,
}

impl APU {
    pub fn new() -> Self {
        Self {
            cpu_cycle: 0,
            frame_counter_cycle: 0,
            frame_counter_reset_delay: None,
            frame_irq_enabled: false,
            frame_irq_flag: false,
            frame_irq_line_low: false,
            frame_irq_line_delay: 0,
            frame_counter_mode_five_step: false,
            frame_irq_event_fired: false,
            frame_irq_assert_window: 0,
            frame_irq_clear_after_cycle: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn tick_cpu_cycle(&mut self) {
        self.cpu_cycle = self.cpu_cycle.wrapping_add(1);

        if self
            .frame_irq_clear_after_cycle
            .is_some_and(|clear_after_cycle| clear_after_cycle < self.cpu_cycle)
        {
            self.frame_irq_flag = false;
            self.frame_irq_line_low = false;
            self.frame_irq_clear_after_cycle = None;
        }

        if !self.frame_irq_enabled
            && self.frame_irq_event_fired
            && self.frame_irq_assert_window == 0
        {
            self.frame_irq_flag = false;
            self.frame_irq_line_low = false;
            self.frame_irq_line_delay = 0;
        }

        if let Some(delay) = self.frame_counter_reset_delay {
            if delay <= 1 {
                self.frame_counter_reset_delay = None;
                self.frame_counter_cycle = 0;
                self.frame_irq_event_fired = false;
                self.frame_irq_assert_window = 0;
            } else {
                self.frame_counter_reset_delay = Some(delay - 1);
            }
            return;
        }

        self.frame_counter_cycle = self.frame_counter_cycle.wrapping_add(1);

        if !self.frame_counter_mode_five_step
            && !self.frame_irq_event_fired
            && self.frame_counter_cycle >= 29_828
        {
            self.frame_irq_event_fired = true;
            self.frame_irq_assert_window = if self.frame_irq_enabled { 3 } else { 2 };
            self.frame_irq_line_delay = if self.frame_irq_enabled { 3 } else { 0 };
        }

        if self.frame_irq_assert_window > 0 {
            self.frame_irq_flag = true;
            self.frame_irq_clear_after_cycle = None;
            self.frame_irq_assert_window -= 1;
        }

        if self.frame_irq_line_delay > 0 {
            self.frame_irq_line_delay -= 1;
        } else if self.frame_irq_enabled && self.frame_irq_flag && self.frame_irq_event_fired {
            self.frame_irq_line_low = true;
        }
    }

    pub fn write_status(&mut self, _data: u8) {}

    pub fn write_frame_counter(&mut self, data: u8) {
        self.write_frame_counter_at_offset(data, 0);
    }

    pub fn read_status(&mut self) -> u8 {
        self.read_status_at_offset(0)
    }

    pub fn write_frame_counter_at_offset(&mut self, data: u8, cycle_offset: u8) {
        self.frame_counter_mode_five_step = (data & 0x80) != 0;
        self.frame_irq_enabled = (data & 0x40) == 0;

        if !self.frame_irq_enabled {
            self.frame_irq_flag = false;
            self.frame_irq_line_low = false;
            self.frame_irq_line_delay = 0;
        }

        let access_cycle = self.cpu_cycle.wrapping_add(cycle_offset as u64);
        Self::trace_frame_irq(format_args!(
            "write $4017 access={} cpu={} data={:02X} enabled={} five_step={} flag_before={} clear_after={:?}",
            access_cycle,
            self.cpu_cycle,
            data,
            self.frame_irq_enabled,
            self.frame_counter_mode_five_step,
            self.frame_irq_flag,
            self.frame_irq_clear_after_cycle
        ));
        let reset_delay = if access_cycle & 1 == 0 { 3 } else { 4 };
        self.frame_counter_reset_delay = Some(reset_delay);
        self.frame_irq_event_fired = false;
        self.frame_irq_assert_window = 0;
        self.frame_irq_line_low = false;
        self.frame_irq_line_delay = 0;
        self.frame_irq_clear_after_cycle = None;
    }

    pub fn read_status_at_offset(&mut self, cycle_offset: u8) -> u8 {
        let access_cycle = self.cpu_cycle.wrapping_add(cycle_offset as u64);
        self.apply_scheduled_events_until(access_cycle);

        let status = if self.frame_irq_flag { 0x40 } else { 0x00 };
        if self.frame_irq_flag {
            self.frame_irq_clear_after_cycle =
                Some(Self::frame_irq_clear_after_cycle(access_cycle));
        }
        Self::trace_frame_irq(format_args!(
            "read $4015 access={} cpu={} status={:02X} flag_after_read={} clear_after={:?}",
            access_cycle,
            self.cpu_cycle,
            status,
            self.frame_irq_flag,
            self.frame_irq_clear_after_cycle
        ));
        status
    }

    pub fn irq_line(&self) -> bool {
        self.frame_irq_line_low && self.frame_irq_enabled && !self.frame_counter_mode_five_step
    }

    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u64(self.cpu_cycle);
        writer.write_u32(self.frame_counter_cycle);
        match self.frame_counter_reset_delay {
            Some(delay) => {
                writer.write_bool(true);
                writer.write_u8(delay);
            }
            None => writer.write_bool(false),
        }
        writer.write_bool(self.frame_irq_enabled);
        writer.write_bool(self.frame_irq_flag);
        writer.write_bool(self.frame_irq_line_low);
        writer.write_u8(self.frame_irq_line_delay);
        writer.write_bool(self.frame_counter_mode_five_step);
        writer.write_bool(self.frame_irq_event_fired);
        writer.write_u8(self.frame_irq_assert_window);
        match self.frame_irq_clear_after_cycle {
            Some(cycle) => {
                writer.write_bool(true);
                writer.write_u64(cycle);
            }
            None => writer.write_bool(false),
        }
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        self.cpu_cycle = reader.read_u64()?;
        self.frame_counter_cycle = reader.read_u32()?;
        self.frame_counter_reset_delay = if reader.read_bool()? {
            Some(reader.read_u8()?)
        } else {
            None
        };
        self.frame_irq_enabled = reader.read_bool()?;
        self.frame_irq_flag = reader.read_bool()?;
        self.frame_irq_line_low = reader.read_bool()?;
        self.frame_irq_line_delay = reader.read_u8()?;
        self.frame_counter_mode_five_step = reader.read_bool()?;
        self.frame_irq_event_fired = reader.read_bool()?;
        self.frame_irq_assert_window = reader.read_u8()?;
        self.frame_irq_clear_after_cycle = if reader.read_bool()? {
            Some(reader.read_u64()?)
        } else {
            None
        };
        Ok(())
    }

    fn apply_scheduled_events_until(&mut self, access_cycle: u64) {
        if self
            .frame_irq_clear_after_cycle
            .is_some_and(|clear_after_cycle| access_cycle > clear_after_cycle)
        {
            self.frame_irq_flag = false;
            self.frame_irq_line_low = false;
            self.frame_irq_line_delay = 0;
            self.frame_irq_clear_after_cycle = None;
        }
    }

    fn frame_irq_clear_after_cycle(access_cycle: u64) -> u64 {
        if access_cycle & 1 == 0 {
            access_cycle + 1
        } else {
            access_cycle
        }
    }

    #[cfg(test)]
    fn trace_frame_irq(args: std::fmt::Arguments<'_>) {
        if std::env::var_os("NES_TRACE_FRAME_IRQ").is_some() {
            eprintln!("{args}");
        }
    }

    #[cfg(not(test))]
    fn trace_frame_irq(_args: std::fmt::Arguments<'_>) {}
}

impl Default for APU {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::APU;

    fn apu_with_pending_frame_irq() -> APU {
        let mut apu = APU::new();
        apu.frame_irq_enabled = true;
        apu.frame_irq_flag = true;
        apu
    }

    #[test]
    fn read_4015_on_put_cycle_clears_before_following_get_cycle() {
        let mut apu = apu_with_pending_frame_irq();

        assert_eq!(apu.read_status_at_offset(5) & 0x40, 0x40);
        assert_eq!(apu.read_status_at_offset(6) & 0x40, 0x00);
    }

    #[test]
    fn read_4015_on_get_cycle_stays_set_through_following_put_cycle() {
        let mut apu = apu_with_pending_frame_irq();

        assert_eq!(apu.read_status_at_offset(6) & 0x40, 0x40);
        assert_eq!(apu.read_status_at_offset(7) & 0x40, 0x40);
        assert_eq!(apu.read_status_at_offset(8) & 0x40, 0x00);
    }

    #[test]
    fn write_4017_on_even_cycle_resets_frame_counter_after_three_cycles() {
        let mut apu = APU::new();

        apu.write_frame_counter_at_offset(0x00, 6);

        assert_eq!(apu.frame_counter_reset_delay, Some(3));
    }

    #[test]
    fn write_4017_on_odd_cycle_resets_frame_counter_after_four_cycles() {
        let mut apu = APU::new();

        apu.write_frame_counter_at_offset(0x00, 5);

        assert_eq!(apu.frame_counter_reset_delay, Some(4));
    }

    #[test]
    fn frame_irq_reassertion_cancels_pending_clear() {
        let mut apu = APU::new();
        apu.frame_irq_enabled = true;
        apu.frame_irq_flag = true;
        apu.frame_irq_assert_window = 1;

        assert_eq!(apu.read_status_at_offset(6) & 0x40, 0x40);
        assert!(apu.frame_irq_clear_after_cycle.is_some());

        apu.tick_cpu_cycle();

        assert_eq!(apu.frame_irq_flag, true);
        assert_eq!(apu.frame_irq_clear_after_cycle, None);
    }

    #[test]
    fn frame_irq_line_goes_low_three_cycles_after_flag_first_sets() {
        let mut apu = APU::new();
        apu.frame_irq_enabled = true;
        apu.frame_counter_cycle = 29_827;

        apu.tick_cpu_cycle();
        assert!(apu.frame_irq_flag);
        assert!(!apu.irq_line());

        apu.tick_cpu_cycle();
        assert!(!apu.irq_line());

        apu.tick_cpu_cycle();
        assert!(!apu.irq_line());

        apu.tick_cpu_cycle();
        assert!(apu.irq_line());
    }
}
