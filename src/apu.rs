use crate::savestate::{SaveStateError, StateReader, StateWriter};

const CPU_CLOCK_HZ_NTSC: u64 = 1_789_773;
const AUDIO_SAMPLE_RATE: u32 = 44_100;
const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];
const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 1, 0, 0, 0],
    [1, 0, 0, 1, 1, 1, 1, 1],
];
const TRIANGLE_TABLE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15,
];

#[derive(Clone, Copy)]
struct PulseChannel {
    enabled: bool,
    duty: u8,
    length_halt: bool,
    constant_volume: bool,
    volume: u8,
    envelope_period: u8,
    envelope_start: bool,
    envelope_divider: u8,
    envelope_decay: u8,
    sweep_enabled: bool,
    sweep_period: u8,
    sweep_negate: bool,
    sweep_shift: u8,
    sweep_reload: bool,
    timer_period: u16,
    timer_value: u16,
    sequence_step: u8,
    length_counter: u8,
}

impl PulseChannel {
    const fn new() -> Self {
        Self {
            enabled: false,
            duty: 0,
            length_halt: false,
            constant_volume: false,
            volume: 0,
            envelope_period: 0,
            envelope_start: false,
            envelope_divider: 0,
            envelope_decay: 0,
            sweep_enabled: false,
            sweep_period: 0,
            sweep_negate: false,
            sweep_shift: 0,
            sweep_reload: false,
            timer_period: 0,
            timer_value: 0,
            sequence_step: 0,
            length_counter: 0,
        }
    }

    fn write_control(&mut self, data: u8) {
        self.duty = (data >> 6) & 0x03;
        self.length_halt = (data & 0x20) != 0;
        self.constant_volume = (data & 0x10) != 0;
        self.volume = data & 0x0F;
        self.envelope_period = data & 0x0F;
        self.envelope_start = true;
    }

    fn write_sweep(&mut self, data: u8) {
        self.sweep_enabled = (data & 0x80) != 0;
        self.sweep_period = (data >> 4) & 0x07;
        self.sweep_negate = (data & 0x08) != 0;
        self.sweep_shift = data & 0x07;
        self.sweep_reload = true;
    }

    fn write_timer_low(&mut self, data: u8) {
        self.timer_period = (self.timer_period & 0x0700) | u16::from(data);
    }

    fn write_timer_high(&mut self, data: u8) {
        self.timer_period = (self.timer_period & 0x00FF) | (u16::from(data & 0x07) << 8);
        self.timer_value = self.timer_period;
        self.sequence_step = 0;
        self.envelope_start = true;
        if self.enabled {
            self.length_counter = LENGTH_TABLE[(data >> 3) as usize];
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length_counter = 0;
        }
    }

    fn clock_timer(&mut self) {
        if self.timer_value == 0 {
            self.timer_value = self.timer_period;
            self.sequence_step = (self.sequence_step + 1) & 0x07;
        } else {
            self.timer_value -= 1;
        }
    }

    fn clock_envelope(&mut self) {
        if self.envelope_start {
            self.envelope_start = false;
            self.envelope_decay = 15;
            self.envelope_divider = self.envelope_period;
            return;
        }

        if self.envelope_divider == 0 {
            self.envelope_divider = self.envelope_period;
            if self.envelope_decay == 0 {
                if self.length_halt {
                    self.envelope_decay = 15;
                }
            } else {
                self.envelope_decay -= 1;
            }
        } else {
            self.envelope_divider -= 1;
        }
    }

    fn clock_length_counter(&mut self) {
        if !self.length_halt && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    fn clock_sweep(&mut self) {
        if self.sweep_reload {
            self.sweep_reload = false;
            return;
        }

        if self.sweep_enabled && self.sweep_shift != 0 {
            let change = self.timer_period >> self.sweep_shift;
            let new_period = if self.sweep_negate {
                self.timer_period.wrapping_sub(change)
            } else {
                self.timer_period.wrapping_add(change)
            };
            self.timer_period = new_period & 0x07FF;
        }
    }

    fn output(&self) -> f32 {
        if !self.enabled || self.length_counter == 0 || self.timer_period < 8 {
            return 0.0;
        }

        if DUTY_TABLE[self.duty as usize][self.sequence_step as usize] == 0 {
            return 0.0;
        }

        let volume = if self.constant_volume {
            self.volume
        } else {
            self.envelope_decay
        };
        f32::from(volume)
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.enabled);
        writer.write_u8(self.duty);
        writer.write_bool(self.length_halt);
        writer.write_bool(self.constant_volume);
        writer.write_u8(self.volume);
        writer.write_u8(self.envelope_period);
        writer.write_bool(self.envelope_start);
        writer.write_u8(self.envelope_divider);
        writer.write_u8(self.envelope_decay);
        writer.write_bool(self.sweep_enabled);
        writer.write_u8(self.sweep_period);
        writer.write_bool(self.sweep_negate);
        writer.write_u8(self.sweep_shift);
        writer.write_bool(self.sweep_reload);
        writer.write_u16(self.timer_period);
        writer.write_u16(self.timer_value);
        writer.write_u8(self.sequence_step);
        writer.write_u8(self.length_counter);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.enabled = reader.read_bool()?;
        self.duty = reader.read_u8()?;
        self.length_halt = reader.read_bool()?;
        self.constant_volume = reader.read_bool()?;
        self.volume = reader.read_u8()?;
        self.envelope_period = reader.read_u8()?;
        self.envelope_start = reader.read_bool()?;
        self.envelope_divider = reader.read_u8()?;
        self.envelope_decay = reader.read_u8()?;
        self.sweep_enabled = reader.read_bool()?;
        self.sweep_period = reader.read_u8()?;
        self.sweep_negate = reader.read_bool()?;
        self.sweep_shift = reader.read_u8()?;
        self.sweep_reload = reader.read_bool()?;
        self.timer_period = reader.read_u16()?;
        self.timer_value = reader.read_u16()?;
        self.sequence_step = reader.read_u8()?;
        self.length_counter = reader.read_u8()?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct TriangleChannel {
    enabled: bool,
    control_flag: bool,
    linear_reload_value: u8,
    linear_reload_flag: bool,
    linear_counter: u8,
    timer_period: u16,
    timer_value: u16,
    sequence_step: u8,
    length_counter: u8,
}

impl TriangleChannel {
    const fn new() -> Self {
        Self {
            enabled: false,
            control_flag: false,
            linear_reload_value: 0,
            linear_reload_flag: false,
            linear_counter: 0,
            timer_period: 0,
            timer_value: 0,
            sequence_step: 0,
            length_counter: 0,
        }
    }

    fn write_control(&mut self, data: u8) {
        self.control_flag = (data & 0x80) != 0;
        self.linear_reload_value = data & 0x7F;
    }

    fn write_timer_low(&mut self, data: u8) {
        self.timer_period = (self.timer_period & 0x0700) | u16::from(data);
    }

    fn write_timer_high(&mut self, data: u8) {
        self.timer_period = (self.timer_period & 0x00FF) | (u16::from(data & 0x07) << 8);
        self.timer_value = self.timer_period;
        self.linear_reload_flag = true;
        if self.enabled {
            self.length_counter = LENGTH_TABLE[(data >> 3) as usize];
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length_counter = 0;
        }
    }

    fn clock_timer(&mut self) {
        if self.timer_value == 0 {
            self.timer_value = self.timer_period;
            if self.length_counter > 0 && self.linear_counter > 0 && self.timer_period >= 2 {
                self.sequence_step = (self.sequence_step + 1) & 0x1F;
            }
        } else {
            self.timer_value -= 1;
        }
    }

    fn clock_length_counter(&mut self) {
        if !self.control_flag && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    fn clock_linear_counter(&mut self) {
        if self.linear_reload_flag {
            self.linear_counter = self.linear_reload_value;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }

        if !self.control_flag {
            self.linear_reload_flag = false;
        }
    }

    fn output(&self) -> f32 {
        if !self.enabled
            || self.length_counter == 0
            || self.linear_counter == 0
            || self.timer_period < 2
        {
            return 0.0;
        }
        f32::from(TRIANGLE_TABLE[self.sequence_step as usize])
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.enabled);
        writer.write_bool(self.control_flag);
        writer.write_u8(self.linear_reload_value);
        writer.write_bool(self.linear_reload_flag);
        writer.write_u8(self.linear_counter);
        writer.write_u16(self.timer_period);
        writer.write_u16(self.timer_value);
        writer.write_u8(self.sequence_step);
        writer.write_u8(self.length_counter);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.enabled = reader.read_bool()?;
        self.control_flag = reader.read_bool()?;
        self.linear_reload_value = reader.read_u8()?;
        self.linear_reload_flag = reader.read_bool()?;
        self.linear_counter = reader.read_u8()?;
        self.timer_period = reader.read_u16()?;
        self.timer_value = reader.read_u16()?;
        self.sequence_step = reader.read_u8()?;
        self.length_counter = reader.read_u8()?;
        Ok(())
    }
}

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
    pulse1: PulseChannel,
    pulse2: PulseChannel,
    triangle: TriangleChannel,
    audio_sample_accumulator: u64,
    sample_buffer: Vec<f32>,
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
            pulse1: PulseChannel::new(),
            pulse2: PulseChannel::new(),
            triangle: TriangleChannel::new(),
            audio_sample_accumulator: 0,
            sample_buffer: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn tick_cpu_cycle(&mut self) {
        self.cpu_cycle = self.cpu_cycle.wrapping_add(1);

        self.clock_timers();

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
            self.push_audio_samples();
            return;
        }

        self.frame_counter_cycle = self.frame_counter_cycle.wrapping_add(1);
        self.clock_frame_sequencer();
        self.advance_frame_irq_line();
        self.push_audio_samples();
    }

    pub fn write_register_at_offset(&mut self, addr: u16, data: u8, cycle_offset: u8) {
        match addr {
            0x4000 => self.pulse1.write_control(data),
            0x4001 => self.pulse1.write_sweep(data),
            0x4002 => self.pulse1.write_timer_low(data),
            0x4003 => self.pulse1.write_timer_high(data),
            0x4004 => self.pulse2.write_control(data),
            0x4005 => self.pulse2.write_sweep(data),
            0x4006 => self.pulse2.write_timer_low(data),
            0x4007 => self.pulse2.write_timer_high(data),
            0x4008 => self.triangle.write_control(data),
            0x400A => self.triangle.write_timer_low(data),
            0x400B => self.triangle.write_timer_high(data),
            0x4015 => self.write_status(data),
            0x4017 => self.write_frame_counter_at_offset(data, cycle_offset),
            _ => {}
        }
    }

    pub fn write_status(&mut self, data: u8) {
        self.pulse1.set_enabled((data & 0x01) != 0);
        self.pulse2.set_enabled((data & 0x02) != 0);
        self.triangle.set_enabled((data & 0x04) != 0);
    }

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

        if self.frame_counter_mode_five_step {
            self.clock_quarter_frame();
            self.clock_half_frame();
        }
    }

    pub fn read_status_at_offset(&mut self, cycle_offset: u8) -> u8 {
        let access_cycle = self.cpu_cycle.wrapping_add(cycle_offset as u64);
        self.apply_scheduled_events_until(access_cycle);

        let mut status = 0;
        if self.pulse1.length_counter > 0 {
            status |= 0x01;
        }
        if self.pulse2.length_counter > 0 {
            status |= 0x02;
        }
        if self.triangle.length_counter > 0 {
            status |= 0x04;
        }
        if self.frame_irq_flag {
            status |= 0x40;
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

    pub fn sample_rate(&self) -> u32 {
        AUDIO_SAMPLE_RATE
    }

    pub fn audio_samples(&self) -> &[f32] {
        &self.sample_buffer
    }

    pub fn clear_audio_samples(&mut self) {
        self.sample_buffer.clear();
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
        self.pulse1.save_state(writer);
        self.pulse2.save_state(writer);
        self.triangle.save_state(writer);
        writer.write_u64(self.audio_sample_accumulator);
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
        self.pulse1.load_state(reader)?;
        self.pulse2.load_state(reader)?;
        self.triangle.load_state(reader)?;
        self.audio_sample_accumulator = reader.read_u64()?;
        self.sample_buffer.clear();
        Ok(())
    }

    fn clock_timers(&mut self) {
        if self.cpu_cycle & 1 == 0 {
            self.pulse1.clock_timer();
            self.pulse2.clock_timer();
        }
        self.triangle.clock_timer();
    }

    fn clock_frame_sequencer(&mut self) {
        let cycle = self.frame_counter_cycle;
        if self.frame_counter_mode_five_step {
            if matches!(cycle, 7_457 | 14_913 | 22_371 | 37_281) {
                self.clock_quarter_frame();
            }
            if matches!(cycle, 14_913 | 37_281) {
                self.clock_half_frame();
            }
            if cycle >= 37_282 {
                self.frame_counter_cycle = 0;
            }
            return;
        }

        if matches!(cycle, 7_457 | 14_913 | 22_371 | 29_829) {
            self.clock_quarter_frame();
        }
        if matches!(cycle, 14_913 | 29_829) {
            self.clock_half_frame();
        }

        if !self.frame_irq_event_fired && cycle >= 29_828 {
            self.frame_irq_event_fired = true;
            self.frame_irq_assert_window = if self.frame_irq_enabled { 3 } else { 2 };
            self.frame_irq_line_delay = if self.frame_irq_enabled { 3 } else { 0 };
        }

        if cycle >= 29_830 {
            self.frame_counter_cycle = 0;
        }
    }

    fn clock_quarter_frame(&mut self) {
        self.pulse1.clock_envelope();
        self.pulse2.clock_envelope();
        self.triangle.clock_linear_counter();
    }

    fn clock_half_frame(&mut self) {
        self.pulse1.clock_length_counter();
        self.pulse2.clock_length_counter();
        self.triangle.clock_length_counter();
        self.pulse1.clock_sweep();
        self.pulse2.clock_sweep();
    }

    fn advance_frame_irq_line(&mut self) {
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

    fn push_audio_samples(&mut self) {
        self.audio_sample_accumulator += u64::from(AUDIO_SAMPLE_RATE);
        while self.audio_sample_accumulator >= CPU_CLOCK_HZ_NTSC {
            self.audio_sample_accumulator -= CPU_CLOCK_HZ_NTSC;
            self.sample_buffer.push(self.mix_sample());
        }
    }

    fn mix_sample(&self) -> f32 {
        let pulse_sum = self.pulse1.output() + self.pulse2.output();
        let triangle = self.triangle.output();

        let pulse_out = if pulse_sum == 0.0 {
            0.0
        } else {
            95.88 / ((8128.0 / pulse_sum) + 100.0)
        };
        let tnd_input = triangle / 8227.0;
        let tnd_out = if tnd_input == 0.0 {
            0.0
        } else {
            159.79 / ((1.0 / tnd_input) + 100.0)
        };
        (pulse_out + tnd_out) * 0.8
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

    #[test]
    fn pulse_channel_generates_non_zero_audio_samples() {
        let mut apu = APU::new();
        apu.write_register_at_offset(0x4015, 0x01, 0);
        apu.write_register_at_offset(0x4000, 0x1F, 0);
        apu.write_register_at_offset(0x4002, 0x20, 0);
        apu.write_register_at_offset(0x4003, 0x08, 0);

        for _ in 0..10_000 {
            apu.tick_cpu_cycle();
        }

        assert!(!apu.audio_samples().is_empty());
        assert!(
            apu.audio_samples()
                .iter()
                .any(|sample| sample.abs() > 0.0001)
        );
    }

    #[test]
    fn triangle_channel_generates_non_zero_audio_samples() {
        let mut apu = APU::new();
        apu.write_register_at_offset(0x4015, 0x04, 0);
        apu.write_register_at_offset(0x4008, 0x8F, 0);
        apu.write_register_at_offset(0x400A, 0x10, 0);
        apu.write_register_at_offset(0x400B, 0x08, 0);

        for _ in 0..8_000 {
            apu.tick_cpu_cycle();
        }

        assert!(apu.triangle.linear_counter > 0);
        assert!(apu.triangle.length_counter > 0);
        assert!(apu.triangle.output() > 0.0);

        apu.clear_audio_samples();
        for _ in 0..512 {
            apu.tick_cpu_cycle();
        }

        assert!(!apu.audio_samples().is_empty());
        assert!(
            apu.audio_samples()
                .iter()
                .any(|sample| sample.abs() > 0.0001)
        );
    }
}
