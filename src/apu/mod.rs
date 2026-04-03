use crate::savestate::{SaveStateError, StateReader, StateWriter};
use std::collections::VecDeque;
use std::f32::consts::PI;
use std::sync::OnceLock;

const CPU_CLOCK_HZ_NTSC: u64 = 1_789_773;
const AUDIO_SAMPLE_RATE: u32 = 44_100;
const AUDIO_HIGHPASS_1_CUTOFF_HZ: f32 = 90.0;
const AUDIO_HIGHPASS_2_CUTOFF_HZ: f32 = 440.0;
const AUDIO_LOWPASS_CUTOFF_HZ: f32 = 14_000.0;
const AUDIO_OUTPUT_GAIN: f32 = 0.9;
const AUDIO_RESAMPLER_TAPS: usize = 32;
const AUDIO_RESAMPLER_PHASES: usize = 256;
const AUDIO_RESAMPLER_HALF_TAPS: usize = AUDIO_RESAMPLER_TAPS / 2;
const AUDIO_RESAMPLER_STEP: f64 = CPU_CLOCK_HZ_NTSC as f64 / AUDIO_SAMPLE_RATE as f64;
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
static AUDIO_RESAMPLER_KERNEL: OnceLock<[[f32; AUDIO_RESAMPLER_TAPS]; AUDIO_RESAMPLER_PHASES]> =
    OnceLock::new();

#[derive(Clone, Copy)]
struct PulseChannel {
    enabled: bool,
    ones_complement_negate: bool,
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
    sweep_divider: u8,
    timer_period: u16,
    timer_value: u16,
    sequence_step: u8,
    length_counter: u8,
}

impl PulseChannel {
    const fn new(ones_complement_negate: bool) -> Self {
        Self {
            enabled: false,
            ones_complement_negate,
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
            sweep_divider: 0,
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
        let divider_zero = self.sweep_divider == 0;

        if divider_zero
            && self.sweep_enabled
            && self.sweep_shift != 0
            && !self.sweep_mutes_channel()
        {
            self.timer_period = self.sweep_target_period();
        }

        if self.sweep_reload || divider_zero {
            self.sweep_divider = self.sweep_period;
            self.sweep_reload = false;
        } else {
            self.sweep_divider -= 1;
        }
    }

    fn output(&self) -> f32 {
        if !self.enabled
            || self.length_counter == 0
            || self.timer_period < 8
            || self.sweep_mutes_channel()
        {
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

    fn sweep_target_period(&self) -> u16 {
        let change = self.timer_period >> self.sweep_shift;
        if self.sweep_negate {
            let extra = u16::from(self.ones_complement_negate);
            self.timer_period.wrapping_sub(change).wrapping_sub(extra)
        } else {
            self.timer_period.wrapping_add(change)
        }
    }

    fn sweep_mutes_channel(&self) -> bool {
        self.timer_period < 8
            || (self.sweep_shift != 0 && !self.sweep_negate && self.sweep_target_period() > 0x07FF)
    }

    fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.enabled);
        writer.write_bool(self.ones_complement_negate);
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
        writer.write_u8(self.sweep_divider);
        writer.write_u16(self.timer_period);
        writer.write_u16(self.timer_value);
        writer.write_u8(self.sequence_step);
        writer.write_u8(self.length_counter);
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.enabled = reader.read_bool()?;
        self.ones_complement_negate = reader.read_bool()?;
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
        self.sweep_divider = reader.read_u8()?;
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
pub(crate) enum DmcDmaKind {
    Load,
    Reload,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DmcDmaRequest {
    pub addr: u16,
    pub kind: DmcDmaKind,
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
    pending_dma_kind: DmcDmaKind,
}

impl DmcChannel {
    const fn new() -> Self {
        Self {
            enabled: false,
            irq_enabled: false,
            irq_flag: false,
            loop_flag: false,
            rate_index: 0,
            timer_value: DMC_RATE_TABLE[0] - 1,
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
            pending_dma_kind: DmcDmaKind::Load,
        }
    }

    fn write_control(&mut self, data: u8) {
        self.irq_enabled = (data & 0x80) != 0;
        if !self.irq_enabled {
            self.irq_flag = false;
        }
        self.loop_flag = (data & 0x40) != 0;
        self.rate_index = data & 0x0F;
        if !self.enabled && self.bytes_remaining == 0 && self.sample_buffer.is_none() {
            self.timer_value = DMC_RATE_TABLE[self.rate_index as usize] - 1;
        }
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
            self.request_dma_if_needed(DmcDmaKind::Load);
        }
    }

    fn clock_timer(&mut self) {
        if self.timer_value == 0 {
            self.timer_value = DMC_RATE_TABLE[self.rate_index as usize] - 1;
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
                kind: self.pending_dma_kind,
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
                self.request_dma_if_needed(DmcDmaKind::Reload);
            } else if self.irq_enabled {
                self.irq_flag = true;
            }
        } else {
            self.request_dma_if_needed(DmcDmaKind::Reload);
        }
    }

    fn restart_sample(&mut self) {
        self.current_address = self.sample_address;
        self.bytes_remaining = self.sample_length;
    }

    fn request_dma_if_needed(&mut self, kind: DmcDmaKind) {
        if self.enabled && self.bytes_remaining > 0 && self.sample_buffer.is_none() {
            self.pending_dma = true;
            self.pending_dma_kind = kind;
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
                self.request_dma_if_needed(DmcDmaKind::Reload);
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
        writer.write_u8(match self.pending_dma_kind {
            DmcDmaKind::Load => 0,
            DmcDmaKind::Reload => 1,
        });
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
        self.pending_dma_kind = match reader.read_u8()? {
            0 => DmcDmaKind::Load,
            1 => DmcDmaKind::Reload,
            _ => return Err(SaveStateError::InvalidData("invalid DMC DMA kind")),
        };
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
            pulse1: PulseChannel::new(true),
            pulse2: PulseChannel::new(false),
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
            self.irq_line_delay = if self.irq_enabled { 4 } else { 0 };
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

struct OnePoleHighPass {
    alpha: f32,
    last_input: f32,
    last_output: f32,
}

impl OnePoleHighPass {
    fn new(cutoff_hz: f32) -> Self {
        let rc = 1.0 / (2.0 * PI * cutoff_hz);
        let dt = 1.0 / AUDIO_SAMPLE_RATE as f32;
        Self {
            alpha: rc / (rc + dt),
            last_input: 0.0,
            last_output: 0.0,
        }
    }

    fn reset(&mut self) {
        self.last_input = 0.0;
        self.last_output = 0.0;
    }

    fn process(&mut self, input: f32) -> f32 {
        let output = self.alpha * (self.last_output + input - self.last_input);
        self.last_input = input;
        self.last_output = output;
        output
    }
}

struct OnePoleLowPass {
    alpha: f32,
    last_output: f32,
}

impl OnePoleLowPass {
    fn new(cutoff_hz: f32) -> Self {
        let rc = 1.0 / (2.0 * PI * cutoff_hz);
        let dt = 1.0 / AUDIO_SAMPLE_RATE as f32;
        Self {
            alpha: dt / (rc + dt),
            last_output: 0.0,
        }
    }

    fn reset(&mut self) {
        self.last_output = 0.0;
    }

    fn process(&mut self, input: f32) -> f32 {
        self.last_output += self.alpha * (input - self.last_output);
        self.last_output
    }
}

struct AudioOutputFilter {
    highpass_90hz: OnePoleHighPass,
    highpass_440hz: OnePoleHighPass,
    lowpass_14khz: OnePoleLowPass,
}

impl AudioOutputFilter {
    fn new() -> Self {
        Self {
            highpass_90hz: OnePoleHighPass::new(AUDIO_HIGHPASS_1_CUTOFF_HZ),
            highpass_440hz: OnePoleHighPass::new(AUDIO_HIGHPASS_2_CUTOFF_HZ),
            lowpass_14khz: OnePoleLowPass::new(AUDIO_LOWPASS_CUTOFF_HZ),
        }
    }

    fn reset(&mut self) {
        self.highpass_90hz.reset();
        self.highpass_440hz.reset();
        self.lowpass_14khz.reset();
    }

    fn process(&mut self, input: f32) -> f32 {
        let sample = self.highpass_90hz.process(input);
        let sample = self.highpass_440hz.process(sample);
        self.lowpass_14khz.process(sample)
    }
}

struct AudioResampler {
    history: VecDeque<f32>,
    history_start_index: i64,
    latest_input_index: i64,
    next_output_position: f64,
    output_filter: AudioOutputFilter,
}

impl AudioResampler {
    fn new() -> Self {
        let mut resampler = Self {
            history: VecDeque::new(),
            history_start_index: 0,
            latest_input_index: -1,
            next_output_position: 0.0,
            output_filter: AudioOutputFilter::new(),
        };
        resampler.reset();
        resampler
    }

    fn reset(&mut self) {
        self.history.clear();
        self.history.resize(AUDIO_RESAMPLER_HALF_TAPS - 1, 0.0);
        self.history_start_index = -((AUDIO_RESAMPLER_HALF_TAPS as i64) - 1);
        self.latest_input_index = -1;
        self.next_output_position = 0.0;
        self.output_filter.reset();
    }

    fn push_source_sample(&mut self, sample: f32, output: &mut Vec<f32>) {
        self.latest_input_index += 1;
        self.history.push_back(sample);

        while self.can_emit_sample() {
            let sample = self.current_output_sample();
            output.push((self.output_filter.process(sample) * AUDIO_OUTPUT_GAIN).clamp(-1.0, 1.0));
            self.next_output_position += AUDIO_RESAMPLER_STEP;
            self.discard_consumed_history();
        }
    }

    fn can_emit_sample(&self) -> bool {
        let (_, window_end) = Self::window_bounds(self.next_output_position);
        self.latest_input_index >= window_end
    }

    fn current_output_sample(&self) -> f32 {
        let base_index = self.next_output_position.floor() as i64;
        let frac = ((self.next_output_position - base_index as f64) as f32)
            .clamp(0.0, 1.0 - (1.0 / AUDIO_RESAMPLER_PHASES as f32));
        let phase =
            ((frac * AUDIO_RESAMPLER_PHASES as f32) as usize).min(AUDIO_RESAMPLER_PHASES - 1);
        let kernel = &audio_resampler_kernel()[phase];
        let window_start = base_index - (AUDIO_RESAMPLER_HALF_TAPS as i64 - 1);

        let mut acc = 0.0;
        for (tap, coeff) in kernel.iter().enumerate() {
            let sample_index = (window_start + tap as i64 - self.history_start_index) as usize;
            acc += self.history[sample_index] * coeff;
        }
        acc
    }

    fn discard_consumed_history(&mut self) {
        let (next_window_start, _) = Self::window_bounds(self.next_output_position);
        let discard_upto = next_window_start.min(self.latest_input_index + 1);
        while self.history_start_index < discard_upto {
            let _ = self.history.pop_front();
            self.history_start_index += 1;
        }
    }

    fn window_bounds(position: f64) -> (i64, i64) {
        let base_index = position.floor() as i64;
        (
            base_index - (AUDIO_RESAMPLER_HALF_TAPS as i64 - 1),
            base_index + AUDIO_RESAMPLER_HALF_TAPS as i64,
        )
    }
}

fn audio_resampler_kernel() -> &'static [[f32; AUDIO_RESAMPLER_TAPS]; AUDIO_RESAMPLER_PHASES] {
    AUDIO_RESAMPLER_KERNEL.get_or_init(|| {
        let mut kernel = [[0.0; AUDIO_RESAMPLER_TAPS]; AUDIO_RESAMPLER_PHASES];
        let cutoff = 0.5 * (AUDIO_SAMPLE_RATE as f32 / CPU_CLOCK_HZ_NTSC as f32) * 0.95;

        for (phase, taps) in kernel.iter_mut().enumerate() {
            let frac = phase as f32 / AUDIO_RESAMPLER_PHASES as f32;
            let mut sum = 0.0;

            for (tap, coeff) in taps.iter_mut().enumerate() {
                let x = tap as f32 - (AUDIO_RESAMPLER_HALF_TAPS as f32 - 1.0) - frac;
                let sinc_arg = 2.0 * cutoff * x;
                let sinc = if sinc_arg.abs() < f32::EPSILON {
                    1.0
                } else {
                    (PI * sinc_arg).sin() / (PI * sinc_arg)
                };
                let window_position = tap as f32 / (AUDIO_RESAMPLER_TAPS - 1) as f32;
                let window = 0.42 - (0.5 * (2.0 * PI * window_position).cos())
                    + (0.08 * (4.0 * PI * window_position).cos());
                *coeff = 2.0 * cutoff * sinc * window;
                sum += *coeff;
            }

            if sum != 0.0 {
                for coeff in taps.iter_mut() {
                    *coeff /= sum;
                }
            }
        }

        kernel
    })
}

pub struct APU {
    cpu_cycle: u64,
    frame_counter: FrameCounter,
    channels: ApuChannels,
    resampler: AudioResampler,
    sample_buffer: Vec<f32>,
}

impl APU {
    pub fn new() -> Self {
        Self {
            cpu_cycle: 0,
            frame_counter: FrameCounter::new(),
            channels: ApuChannels::new(),
            resampler: AudioResampler::new(),
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
        let mixed = self.channels.mix_sample();
        let resampler = &mut self.resampler;
        let sample_buffer = &mut self.sample_buffer;
        resampler.push_source_sample(mixed, sample_buffer);
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
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        self.cpu_cycle = reader.read_u64()?;
        self.frame_counter.load_state(reader)?;
        self.channels.load_state(reader)?;
        self.resampler.reset();
        self.sample_buffer.clear();
        Ok(())
    }

    pub(crate) fn take_dmc_dma_request(&mut self) -> Option<DmcDmaRequest> {
        self.channels.dmc.take_dma_request()
    }

    pub(crate) fn submit_dmc_dma_sample(&mut self, data: u8) {
        self.channels.dmc.submit_dma_sample(data);
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
mod tests;
