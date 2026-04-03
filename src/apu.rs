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
const NOISE_PERIOD_TABLE: [u16; 16] = [
    4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
];
const DMC_RATE_TABLE: [u16; 16] = [
    428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
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

#[derive(Clone, Copy)]
struct NoiseChannel {
    enabled: bool,
    length_halt: bool,
    constant_volume: bool,
    volume: u8,
    envelope_period: u8,
    envelope_start: bool,
    envelope_divider: u8,
    envelope_decay: u8,
    mode_loop: bool,
    period_index: u8,
    timer_value: u16,
    length_counter: u8,
    shift_register: u16,
}

impl NoiseChannel {
    const fn new() -> Self {
        Self {
            enabled: false,
            length_halt: false,
            constant_volume: false,
            volume: 0,
            envelope_period: 0,
            envelope_start: false,
            envelope_divider: 0,
            envelope_decay: 0,
            mode_loop: false,
            period_index: 0,
            timer_value: 0,
            length_counter: 0,
            shift_register: 1,
        }
    }

    fn write_control(&mut self, data: u8) {
        self.length_halt = (data & 0x20) != 0;
        self.constant_volume = (data & 0x10) != 0;
        self.volume = data & 0x0F;
        self.envelope_period = data & 0x0F;
        self.envelope_start = true;
    }

    fn write_period(&mut self, data: u8) {
        self.mode_loop = (data & 0x80) != 0;
        self.period_index = data & 0x0F;
    }

    fn write_length(&mut self, data: u8) {
        if self.enabled {
            self.length_counter = LENGTH_TABLE[(data >> 3) as usize];
        }
        self.envelope_start = true;
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length_counter = 0;
        }
    }

    fn clock_timer(&mut self) {
        if self.timer_value == 0 {
            self.timer_value = NOISE_PERIOD_TABLE[self.period_index as usize];
            let tap = if self.mode_loop { 6 } else { 1 };
            let feedback = (self.shift_register ^ (self.shift_register >> tap)) & 0x01;
            self.shift_register = (self.shift_register >> 1) | (feedback << 14);
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

    fn output(&self) -> f32 {
        if !self.enabled || self.length_counter == 0 || (self.shift_register & 0x01) != 0 {
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
        writer.write_bool(self.length_halt);
        writer.write_bool(self.constant_volume);
        writer.write_u8(self.volume);
        writer.write_u8(self.envelope_period);
        writer.write_bool(self.envelope_start);
        writer.write_u8(self.envelope_divider);
        writer.write_u8(self.envelope_decay);
        writer.write_bool(self.mode_loop);
        writer.write_u8(self.period_index);
        writer.write_u16(self.timer_value);
        writer.write_u8(self.length_counter);
        writer.write_u16(self.shift_register);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.enabled = reader.read_bool()?;
        self.length_halt = reader.read_bool()?;
        self.constant_volume = reader.read_bool()?;
        self.volume = reader.read_u8()?;
        self.envelope_period = reader.read_u8()?;
        self.envelope_start = reader.read_bool()?;
        self.envelope_divider = reader.read_u8()?;
        self.envelope_decay = reader.read_u8()?;
        self.mode_loop = reader.read_bool()?;
        self.period_index = reader.read_u8()?;
        self.timer_value = reader.read_u16()?;
        self.length_counter = reader.read_u8()?;
        self.shift_register = reader.read_u16()?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DmcDmaRequest {
    pub addr: u16,
}

#[derive(Clone, Copy)]
struct DmcChannel {
    enabled: bool,
    irq_enabled: bool,
    irq_flag: bool,
    loop_flag: bool,
    rate_index: u8,
    timer_value: u16,
    output_level: u8,
    sample_address: u16,
    sample_length: u16,
    current_address: u16,
    bytes_remaining: u16,
    shift_register: u8,
    bits_remaining: u8,
    sample_buffer: Option<u8>,
    silent: bool,
    pending_dma: bool,
}

impl DmcChannel {
    const fn new() -> Self {
        Self {
            enabled: false,
            irq_enabled: false,
            irq_flag: false,
            loop_flag: false,
            rate_index: 0,
            timer_value: DMC_RATE_TABLE[0],
            output_level: 0,
            sample_address: 0xC000,
            sample_length: 1,
            current_address: 0xC000,
            bytes_remaining: 0,
            shift_register: 0,
            bits_remaining: 8,
            sample_buffer: None,
            silent: true,
            pending_dma: false,
        }
    }

    fn write_control(&mut self, data: u8) {
        self.irq_enabled = (data & 0x80) != 0;
        if !self.irq_enabled {
            self.irq_flag = false;
        }
        self.loop_flag = (data & 0x40) != 0;
        self.rate_index = data & 0x0F;
        self.timer_value = DMC_RATE_TABLE[self.rate_index as usize];
    }

    fn write_direct_load(&mut self, data: u8) {
        self.output_level = data & 0x7F;
    }

    fn write_sample_address(&mut self, data: u8) {
        self.sample_address = 0xC000 | (u16::from(data) << 6);
    }

    fn write_sample_length(&mut self, data: u8) {
        self.sample_length = (u16::from(data) << 4) | 0x0001;
    }

    fn write_status_enable(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.irq_flag = false;
        if !enabled {
            self.bytes_remaining = 0;
            self.pending_dma = false;
            return;
        }

        if self.bytes_remaining == 0 {
            self.restart_sample();
            self.request_dma_if_needed();
        }
    }

    fn clock_timer(&mut self) {
        if self.timer_value == 0 {
            self.timer_value = DMC_RATE_TABLE[self.rate_index as usize];
            self.clock_output_unit();
        } else {
            self.timer_value -= 1;
        }
    }

    fn active(&self) -> bool {
        self.bytes_remaining > 0
    }

    fn irq_flag(&self) -> bool {
        self.irq_flag
    }

    fn output(&self) -> f32 {
        f32::from(self.output_level)
    }

    fn take_dma_request(&mut self) -> Option<DmcDmaRequest> {
        if self.pending_dma {
            self.pending_dma = false;
            Some(DmcDmaRequest {
                addr: self.current_address,
            })
        } else {
            None
        }
    }

    fn submit_dma_sample(&mut self, data: u8) {
        self.sample_buffer = Some(data);
        self.current_address = self.current_address.wrapping_add(1);
        if self.current_address < 0x8000 {
            self.current_address = 0x8000;
        }

        if self.bytes_remaining > 0 {
            self.bytes_remaining -= 1;
        }

        if self.bytes_remaining == 0 {
            if self.loop_flag {
                self.restart_sample();
                self.request_dma_if_needed();
            } else if self.irq_enabled {
                self.irq_flag = true;
            }
        } else {
            self.request_dma_if_needed();
        }
    }

    fn restart_sample(&mut self) {
        self.current_address = self.sample_address;
        self.bytes_remaining = self.sample_length;
    }

    fn request_dma_if_needed(&mut self) {
        if self.enabled && self.bytes_remaining > 0 && self.sample_buffer.is_none() {
            self.pending_dma = true;
        }
    }

    fn clock_output_unit(&mut self) {
        if !self.silent {
            if (self.shift_register & 0x01) != 0 {
                if self.output_level <= 125 {
                    self.output_level += 2;
                }
            } else if self.output_level >= 2 {
                self.output_level -= 2;
            }
        }

        self.shift_register >>= 1;
        if self.bits_remaining > 0 {
            self.bits_remaining -= 1;
        }

        if self.bits_remaining == 0 {
            self.bits_remaining = 8;
            if let Some(sample) = self.sample_buffer.take() {
                self.shift_register = sample;
                self.silent = false;
                self.request_dma_if_needed();
            } else {
                self.silent = true;
            }
        }
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.enabled);
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.irq_flag);
        writer.write_bool(self.loop_flag);
        writer.write_u8(self.rate_index);
        writer.write_u16(self.timer_value);
        writer.write_u8(self.output_level);
        writer.write_u16(self.sample_address);
        writer.write_u16(self.sample_length);
        writer.write_u16(self.current_address);
        writer.write_u16(self.bytes_remaining);
        writer.write_u8(self.shift_register);
        writer.write_u8(self.bits_remaining);
        match self.sample_buffer {
            Some(sample) => {
                writer.write_bool(true);
                writer.write_u8(sample);
            }
            None => writer.write_bool(false),
        }
        writer.write_bool(self.silent);
        writer.write_bool(self.pending_dma);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.enabled = reader.read_bool()?;
        self.irq_enabled = reader.read_bool()?;
        self.irq_flag = reader.read_bool()?;
        self.loop_flag = reader.read_bool()?;
        self.rate_index = reader.read_u8()?;
        self.timer_value = reader.read_u16()?;
        self.output_level = reader.read_u8()?;
        self.sample_address = reader.read_u16()?;
        self.sample_length = reader.read_u16()?;
        self.current_address = reader.read_u16()?;
        self.bytes_remaining = reader.read_u16()?;
        self.shift_register = reader.read_u8()?;
        self.bits_remaining = reader.read_u8()?;
        self.sample_buffer = if reader.read_bool()? {
            Some(reader.read_u8()?)
        } else {
            None
        };
        self.silent = reader.read_bool()?;
        self.pending_dma = reader.read_bool()?;
        Ok(())
    }
}

#[derive(Clone, Copy, Default)]
struct FrameCounterEvents {
    quarter_frame: bool,
    half_frame: bool,
}

struct ApuChannels {
    pulse1: PulseChannel,
    pulse2: PulseChannel,
    triangle: TriangleChannel,
    noise: NoiseChannel,
    dmc: DmcChannel,
}

impl ApuChannels {
    const fn new() -> Self {
        Self {
            pulse1: PulseChannel::new(),
            pulse2: PulseChannel::new(),
            triangle: TriangleChannel::new(),
            noise: NoiseChannel::new(),
            dmc: DmcChannel::new(),
        }
    }

    fn write_register(&mut self, addr: u16, data: u8) {
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
            0x400C => self.noise.write_control(data),
            0x400E => self.noise.write_period(data),
            0x400F => self.noise.write_length(data),
            0x4010 => self.dmc.write_control(data),
            0x4011 => self.dmc.write_direct_load(data),
            0x4012 => self.dmc.write_sample_address(data),
            0x4013 => self.dmc.write_sample_length(data),
            _ => {}
        }
    }

    fn write_status(&mut self, data: u8) {
        self.pulse1.set_enabled((data & 0x01) != 0);
        self.pulse2.set_enabled((data & 0x02) != 0);
        self.triangle.set_enabled((data & 0x04) != 0);
        self.noise.set_enabled((data & 0x08) != 0);
        self.dmc.write_status_enable((data & 0x10) != 0);
    }

    fn status_bits(&self) -> u8 {
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
        if self.noise.length_counter > 0 {
            status |= 0x08;
        }
        if self.dmc.active() {
            status |= 0x10;
        }
        if self.dmc.irq_flag() {
            status |= 0x80;
        }
        status
    }

    fn clock_timers(&mut self, cpu_cycle: u64) {
        if cpu_cycle & 1 == 0 {
            self.pulse1.clock_timer();
            self.pulse2.clock_timer();
            self.noise.clock_timer();
        }
        self.triangle.clock_timer();
        self.dmc.clock_timer();
    }

    fn apply_frame_counter_events(&mut self, events: FrameCounterEvents) {
        if events.quarter_frame {
            self.pulse1.clock_envelope();
            self.pulse2.clock_envelope();
            self.triangle.clock_linear_counter();
            self.noise.clock_envelope();
        }
        if events.half_frame {
            self.pulse1.clock_length_counter();
            self.pulse2.clock_length_counter();
            self.triangle.clock_length_counter();
            self.noise.clock_length_counter();
            self.pulse1.clock_sweep();
            self.pulse2.clock_sweep();
        }
    }

    fn mix_sample(&self) -> f32 {
        let pulse_sum = self.pulse1.output() + self.pulse2.output();
        let triangle = self.triangle.output();
        let noise = self.noise.output();
        let dmc = self.dmc.output();

        let pulse_out = if pulse_sum == 0.0 {
            0.0
        } else {
            95.88 / ((8128.0 / pulse_sum) + 100.0)
        };
        let tnd_input = (triangle / 8227.0) + (noise / 12241.0) + (dmc / 22638.0);
        let tnd_out = if tnd_input == 0.0 {
            0.0
        } else {
            159.79 / ((1.0 / tnd_input) + 100.0)
        };
        (pulse_out + tnd_out) * 0.8
    }

    fn save_state(&self, writer: &mut StateWriter) {
        self.pulse1.save_state(writer);
        self.pulse2.save_state(writer);
        self.triangle.save_state(writer);
        self.noise.save_state(writer);
        self.dmc.save_state(writer);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.pulse1.load_state(reader)?;
        self.pulse2.load_state(reader)?;
        self.triangle.load_state(reader)?;
        self.noise.load_state(reader)?;
        self.dmc.load_state(reader)?;
        Ok(())
    }
}

impl Default for ApuChannels {
    fn default() -> Self {
        Self::new()
    }
}

struct FrameCounter {
    cycle: u32,
    reset_delay: Option<u8>,
    irq_enabled: bool,
    irq_flag: bool,
    irq_line_low: bool,
    irq_line_delay: u8,
    mode_five_step: bool,
    irq_event_fired: bool,
    irq_assert_window: u8,
    irq_clear_after_cycle: Option<u64>,
}

impl FrameCounter {
    const fn new() -> Self {
        Self {
            cycle: 0,
            reset_delay: None,
            irq_enabled: false,
            irq_flag: false,
            irq_line_low: false,
            irq_line_delay: 0,
            mode_five_step: false,
            irq_event_fired: false,
            irq_assert_window: 0,
            irq_clear_after_cycle: None,
        }
    }

    fn tick(&mut self, cpu_cycle: u64) -> FrameCounterEvents {
        if self
            .irq_clear_after_cycle
            .is_some_and(|clear_after_cycle| clear_after_cycle < cpu_cycle)
        {
            self.irq_flag = false;
            self.irq_line_low = false;
            self.irq_clear_after_cycle = None;
        }

        if !self.irq_enabled && self.irq_event_fired && self.irq_assert_window == 0 {
            self.irq_flag = false;
            self.irq_line_low = false;
            self.irq_line_delay = 0;
        }

        if let Some(delay) = self.reset_delay {
            if delay <= 1 {
                self.reset_delay = None;
                self.cycle = 0;
                self.irq_event_fired = false;
                self.irq_assert_window = 0;
            } else {
                self.reset_delay = Some(delay - 1);
            }
            return FrameCounterEvents::default();
        }

        self.cycle = self.cycle.wrapping_add(1);
        let events = self.clock_sequencer();
        self.advance_irq_line();
        events
    }

    fn write_register_at_offset(
        &mut self,
        data: u8,
        cycle_offset: u8,
        cpu_cycle: u64,
    ) -> FrameCounterEvents {
        self.mode_five_step = (data & 0x80) != 0;
        self.irq_enabled = (data & 0x40) == 0;

        if !self.irq_enabled {
            self.irq_flag = false;
            self.irq_line_low = false;
            self.irq_line_delay = 0;
        }

        let access_cycle = cpu_cycle.wrapping_add(cycle_offset as u64);
        trace_frame_irq(format_args!(
            "write $4017 access={} cpu={} data={:02X} enabled={} five_step={} flag_before={} clear_after={:?}",
            access_cycle,
            cpu_cycle,
            data,
            self.irq_enabled,
            self.mode_five_step,
            self.irq_flag,
            self.irq_clear_after_cycle
        ));
        self.reset_delay = Some(if access_cycle & 1 == 0 { 3 } else { 4 });
        self.irq_event_fired = false;
        self.irq_assert_window = 0;
        self.irq_line_low = false;
        self.irq_line_delay = 0;
        self.irq_clear_after_cycle = None;

        if self.mode_five_step {
            FrameCounterEvents {
                quarter_frame: true,
                half_frame: true,
            }
        } else {
            FrameCounterEvents::default()
        }
    }

    fn read_status(&mut self, cycle_offset: u8, cpu_cycle: u64, channel_status: u8) -> u8 {
        let access_cycle = cpu_cycle.wrapping_add(cycle_offset as u64);
        self.apply_scheduled_events_until(access_cycle);

        let mut status = channel_status;
        if self.irq_flag {
            status |= 0x40;
            self.irq_clear_after_cycle = Some(Self::frame_irq_clear_after_cycle(access_cycle));
        }
        trace_frame_irq(format_args!(
            "read $4015 access={} cpu={} status={:02X} flag_after_read={} clear_after={:?}",
            access_cycle, cpu_cycle, status, self.irq_flag, self.irq_clear_after_cycle
        ));
        status
    }

    fn irq_line(&self) -> bool {
        self.irq_line_low && self.irq_enabled && !self.mode_five_step
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u32(self.cycle);
        match self.reset_delay {
            Some(delay) => {
                writer.write_bool(true);
                writer.write_u8(delay);
            }
            None => writer.write_bool(false),
        }
        writer.write_bool(self.irq_enabled);
        writer.write_bool(self.irq_flag);
        writer.write_bool(self.irq_line_low);
        writer.write_u8(self.irq_line_delay);
        writer.write_bool(self.mode_five_step);
        writer.write_bool(self.irq_event_fired);
        writer.write_u8(self.irq_assert_window);
        match self.irq_clear_after_cycle {
            Some(cycle) => {
                writer.write_bool(true);
                writer.write_u64(cycle);
            }
            None => writer.write_bool(false),
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.cycle = reader.read_u32()?;
        self.reset_delay = if reader.read_bool()? {
            Some(reader.read_u8()?)
        } else {
            None
        };
        self.irq_enabled = reader.read_bool()?;
        self.irq_flag = reader.read_bool()?;
        self.irq_line_low = reader.read_bool()?;
        self.irq_line_delay = reader.read_u8()?;
        self.mode_five_step = reader.read_bool()?;
        self.irq_event_fired = reader.read_bool()?;
        self.irq_assert_window = reader.read_u8()?;
        self.irq_clear_after_cycle = if reader.read_bool()? {
            Some(reader.read_u64()?)
        } else {
            None
        };
        Ok(())
    }

    fn clock_sequencer(&mut self) -> FrameCounterEvents {
        let cycle = self.cycle;
        let mut events = FrameCounterEvents::default();

        if self.mode_five_step {
            if matches!(cycle, 7_457 | 14_913 | 22_371 | 37_281) {
                events.quarter_frame = true;
            }
            if matches!(cycle, 14_913 | 37_281) {
                events.half_frame = true;
            }
            if cycle >= 37_282 {
                self.cycle = 0;
            }
            return events;
        }

        if matches!(cycle, 7_457 | 14_913 | 22_371 | 29_829) {
            events.quarter_frame = true;
        }
        if matches!(cycle, 14_913 | 29_829) {
            events.half_frame = true;
        }

        if !self.irq_event_fired && cycle >= 29_828 {
            self.irq_event_fired = true;
            self.irq_assert_window = if self.irq_enabled { 3 } else { 2 };
            self.irq_line_delay = if self.irq_enabled { 3 } else { 0 };
        }

        if cycle >= 29_830 {
            self.cycle = 0;
        }

        events
    }

    fn advance_irq_line(&mut self) {
        if self.irq_assert_window > 0 {
            self.irq_flag = true;
            self.irq_clear_after_cycle = None;
            self.irq_assert_window -= 1;
        }

        if self.irq_line_delay > 0 {
            self.irq_line_delay -= 1;
        } else if self.irq_enabled && self.irq_flag && self.irq_event_fired {
            self.irq_line_low = true;
        }
    }

    fn apply_scheduled_events_until(&mut self, access_cycle: u64) {
        if self
            .irq_clear_after_cycle
            .is_some_and(|clear_after_cycle| access_cycle > clear_after_cycle)
        {
            self.irq_flag = false;
            self.irq_line_low = false;
            self.irq_line_delay = 0;
            self.irq_clear_after_cycle = None;
        }
    }

    fn frame_irq_clear_after_cycle(access_cycle: u64) -> u64 {
        if access_cycle & 1 == 0 {
            access_cycle + 1
        } else {
            access_cycle
        }
    }
}

impl Default for FrameCounter {
    fn default() -> Self {
        Self::new()
    }
}

pub struct APU {
    cpu_cycle: u64,
    frame_counter: FrameCounter,
    channels: ApuChannels,
    audio_sample_accumulator: u64,
    sample_buffer: Vec<f32>,
}

impl APU {
    pub fn new() -> Self {
        Self {
            cpu_cycle: 0,
            frame_counter: FrameCounter::new(),
            channels: ApuChannels::new(),
            audio_sample_accumulator: 0,
            sample_buffer: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn tick_cpu_cycle(&mut self) {
        self.cpu_cycle = self.cpu_cycle.wrapping_add(1);
        self.channels.clock_timers(self.cpu_cycle);
        let events = self.frame_counter.tick(self.cpu_cycle);
        self.channels.apply_frame_counter_events(events);
        self.push_audio_samples();
    }

    pub fn write_register_at_offset(&mut self, addr: u16, data: u8, cycle_offset: u8) {
        match addr {
            0x4000..=0x4008 | 0x400A..=0x400F | 0x4010..=0x4013 => {
                self.channels.write_register(addr, data)
            }
            0x4015 => self.write_status(data),
            0x4017 => self.write_frame_counter_at_offset(data, cycle_offset),
            _ => {}
        }
    }

    pub fn write_status(&mut self, data: u8) {
        self.channels.write_status(data);
    }

    pub fn write_frame_counter(&mut self, data: u8) {
        self.write_frame_counter_at_offset(data, 0);
    }

    pub fn read_status(&mut self) -> u8 {
        self.read_status_at_offset(0)
    }

    pub fn write_frame_counter_at_offset(&mut self, data: u8, cycle_offset: u8) {
        let events =
            self.frame_counter
                .write_register_at_offset(data, cycle_offset, self.cpu_cycle);
        self.channels.apply_frame_counter_events(events);
    }

    pub fn read_status_at_offset(&mut self, cycle_offset: u8) -> u8 {
        self.frame_counter
            .read_status(cycle_offset, self.cpu_cycle, self.channels.status_bits())
    }

    pub fn irq_line(&self) -> bool {
        self.frame_counter.irq_line() || self.channels.dmc.irq_flag()
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
        self.frame_counter.save_state(writer);
        self.channels.save_state(writer);
        writer.write_u64(self.audio_sample_accumulator);
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        self.cpu_cycle = reader.read_u64()?;
        self.frame_counter.load_state(reader)?;
        self.channels.load_state(reader)?;
        self.audio_sample_accumulator = reader.read_u64()?;
        self.sample_buffer.clear();
        Ok(())
    }

    pub(crate) fn take_dmc_dma_request(&mut self) -> Option<DmcDmaRequest> {
        self.channels.dmc.take_dma_request()
    }

    pub(crate) fn submit_dmc_dma_sample(&mut self, data: u8) {
        self.channels.dmc.submit_dma_sample(data);
    }

    fn push_audio_samples(&mut self) {
        self.audio_sample_accumulator += u64::from(AUDIO_SAMPLE_RATE);
        while self.audio_sample_accumulator >= CPU_CLOCK_HZ_NTSC {
            self.audio_sample_accumulator -= CPU_CLOCK_HZ_NTSC;
            self.sample_buffer.push(self.channels.mix_sample());
        }
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

impl Default for APU {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::{APU, DmcDmaRequest};

    fn apu_with_pending_frame_irq() -> APU {
        let mut apu = APU::new();
        apu.frame_counter.irq_enabled = true;
        apu.frame_counter.irq_flag = true;
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

        assert_eq!(apu.frame_counter.reset_delay, Some(3));
    }

    #[test]
    fn write_4017_on_odd_cycle_resets_frame_counter_after_four_cycles() {
        let mut apu = APU::new();

        apu.write_frame_counter_at_offset(0x00, 5);

        assert_eq!(apu.frame_counter.reset_delay, Some(4));
    }

    #[test]
    fn frame_irq_reassertion_cancels_pending_clear() {
        let mut apu = APU::new();
        apu.frame_counter.irq_enabled = true;
        apu.frame_counter.irq_flag = true;
        apu.frame_counter.irq_assert_window = 1;

        assert_eq!(apu.read_status_at_offset(6) & 0x40, 0x40);
        assert!(apu.frame_counter.irq_clear_after_cycle.is_some());

        apu.tick_cpu_cycle();

        assert_eq!(apu.frame_counter.irq_flag, true);
        assert_eq!(apu.frame_counter.irq_clear_after_cycle, None);
    }

    #[test]
    fn frame_irq_line_goes_low_three_cycles_after_flag_first_sets() {
        let mut apu = APU::new();
        apu.frame_counter.irq_enabled = true;
        apu.frame_counter.cycle = 29_827;

        apu.tick_cpu_cycle();
        assert!(apu.frame_counter.irq_flag);
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

        assert!(apu.channels.triangle.linear_counter > 0);
        assert!(apu.channels.triangle.length_counter > 0);
        assert!(apu.channels.triangle.output() > 0.0);

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

    #[test]
    fn noise_channel_generates_non_zero_audio_samples() {
        let mut apu = APU::new();
        apu.write_register_at_offset(0x4015, 0x08, 0);
        apu.write_register_at_offset(0x400C, 0x1F, 0);
        apu.write_register_at_offset(0x400E, 0x00, 0);
        apu.write_register_at_offset(0x400F, 0x08, 0);

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
    fn dmc_enable_sets_status_bit_and_exposes_dma_request() {
        let mut apu = APU::new();
        apu.write_register_at_offset(0x4012, 0x34, 0);
        apu.write_register_at_offset(0x4013, 0x01, 0);
        apu.write_register_at_offset(0x4015, 0x10, 0);

        assert_eq!(apu.read_status_at_offset(0) & 0x10, 0x10);
        assert_eq!(
            apu.take_dmc_dma_request(),
            Some(DmcDmaRequest { addr: 0xCD00 })
        );
    }

    #[test]
    fn dmc_direct_load_contributes_to_audio_mix() {
        let mut apu = APU::new();
        apu.write_register_at_offset(0x4011, 0x40, 0);

        for _ in 0..256 {
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
