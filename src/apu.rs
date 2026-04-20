use crate::savestate::{SaveStateError, StateReader, StateWriter};

const CPU_CLOCK_NTSC: f64 = 1_789_773.0;
const CPU_CLOCK_NTSC_F32: f32 = 1_789_773.0;
const DEFAULT_SAMPLE_RATE: u32 = 44_100;
const FRAME_SEQUENCER_DIVIDER: u16 = 7_456;
const DUTY_TABLE: [[u8; 8]; 4] = [
    [0, 1, 0, 0, 0, 0, 0, 0],
    [0, 1, 1, 0, 0, 0, 0, 0],
    [0, 1, 1, 1, 1, 0, 0, 0],
    [1, 0, 0, 1, 1, 1, 1, 1],
];
const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];
const TRIANGLE_TABLE: [u8; 32] = [
    15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12,
    13, 14, 15,
];

#[allow(dead_code)]
#[derive(Clone, Copy)]
pub enum DmcDmaKind {
    Load,
    Reload,
}

#[derive(Clone, Copy)]
pub struct DmcDmaRequest {
    pub addr: u16,
    pub kind: DmcDmaKind,
}

pub trait ExpansionAudioChip {
    fn cpu_write(&mut self, _addr: u16, _data: u8) {}
    #[allow(dead_code)]
    fn cpu_read(&mut self, _addr: u16) -> Option<u8> {
        None
    }
    fn tick_cpu_cycle(&mut self) {}
    fn irq_line(&self) -> bool {
        false
    }
    fn output_sample(&self) -> f32 {
        0.0
    }
}

#[derive(Clone, Copy, Default)]
struct PulseChannel {
    enabled: bool,
    duty: u8,
    seq_step: u8,
    timer_reload: u16,
    timer_counter: u16,
    length_counter: u8,
    length_halt: bool,
    constant_volume: bool,
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
    sweep_mute: bool,
    sweep_negate_extra: u16,
}

impl PulseChannel {
    fn new(is_pulse1: bool) -> Self {
        Self {
            sweep_negate_extra: if is_pulse1 { 0 } else { 1 },
            ..Self::default()
        }
    }

    fn write_control(&mut self, value: u8) {
        self.duty = (value >> 6) & 0x03;
        self.length_halt = (value & 0x20) != 0;
        self.constant_volume = (value & 0x10) != 0;
        self.envelope_period = value & 0x0F;
    }

    fn write_timer_low(&mut self, value: u8) {
        self.timer_reload = (self.timer_reload & 0x0700) | value as u16;
        self.refresh_sweep_mute();
    }

    fn write_timer_high(&mut self, value: u8, length_enabled: bool) {
        self.timer_reload = (self.timer_reload & 0x00FF) | (((value & 0x07) as u16) << 8);
        self.seq_step = 0;
        self.timer_counter = self.timer_reload;
        self.envelope_start = true;
        if length_enabled {
            self.length_counter = LENGTH_TABLE[(value >> 3) as usize];
        }
        self.refresh_sweep_mute();
    }

    fn write_sweep(&mut self, value: u8) {
        self.sweep_enabled = (value & 0x80) != 0;
        self.sweep_period = (value >> 4) & 0x07;
        self.sweep_negate = (value & 0x08) != 0;
        self.sweep_shift = value & 0x07;
        self.sweep_reload = true;
        self.refresh_sweep_mute();
    }

    fn tick_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_reload;
            self.seq_step = (self.seq_step + 1) & 0x07;
        } else {
            self.timer_counter -= 1;
        }
    }

    fn quarter_frame_tick(&mut self) {
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

    fn half_frame_tick(&mut self) {
        if !self.length_halt && self.length_counter > 0 {
            self.length_counter -= 1;
        }
        self.tick_sweep();
    }

    fn set_enabled(&mut self, enabled: bool) {
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

    fn output(&self) -> u8 {
        if !self.enabled
            || self.length_counter == 0
            || self.timer_reload < 8
            || self.timer_reload > 0x07FF
            || self.sweep_mute
        {
            return 0;
        }
        let duty_value = DUTY_TABLE[self.duty as usize][self.seq_step as usize];
        if duty_value == 0 {
            return 0;
        }
        self.envelope_volume()
    }

    fn target_period(&self) -> u16 {
        let change = self.timer_reload >> self.sweep_shift;
        if self.sweep_negate {
            self.timer_reload
                .wrapping_sub(change)
                .wrapping_add(self.sweep_negate_extra)
        } else {
            self.timer_reload.wrapping_add(change)
        }
    }

    fn refresh_sweep_mute(&mut self) {
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
            && self.length_counter > 0
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

#[derive(Clone, Copy, Default)]
struct TriangleChannel {
    enabled: bool,
    timer_reload: u16,
    timer_counter: u16,
    seq_step: u8,
    length_counter: u8,
    control_flag: bool,
    linear_reload_value: u8,
    linear_counter: u8,
    linear_reload_flag: bool,
}

impl TriangleChannel {
    fn write_linear_control(&mut self, value: u8) {
        self.control_flag = (value & 0x80) != 0;
        self.linear_reload_value = value & 0x7F;
    }

    fn write_timer_low(&mut self, value: u8) {
        self.timer_reload = (self.timer_reload & 0x0700) | value as u16;
    }

    fn write_timer_high(&mut self, value: u8, length_enabled: bool) {
        self.timer_reload = (self.timer_reload & 0x00FF) | (((value & 0x07) as u16) << 8);
        if length_enabled {
            self.length_counter = LENGTH_TABLE[(value >> 3) as usize];
        }
        self.linear_reload_flag = true;
    }

    fn tick_timer(&mut self) {
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

    fn quarter_frame_tick(&mut self) {
        if self.linear_reload_flag {
            self.linear_counter = self.linear_reload_value;
        } else if self.linear_counter > 0 {
            self.linear_counter -= 1;
        }

        if !self.control_flag {
            self.linear_reload_flag = false;
        }
    }

    fn half_frame_tick(&mut self) {
        if !self.control_flag && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.length_counter = 0;
        }
    }

    fn output(&self) -> u8 {
        if !self.enabled || self.timer_reload < 2 {
            return 0;
        }
        // Hardware keeps triangle DAC at the current sequencer step when the
        // length/linear gate closes; it does not hard-drop to zero.
        TRIANGLE_TABLE[self.seq_step as usize]
    }
}

#[derive(Clone, Copy)]
struct NoiseChannel {
    enabled: bool,
    mode: bool,
    timer_reload: u16,
    timer_counter: u16,
    length_counter: u8,
    length_halt: bool,
    constant_volume: bool,
    envelope_period: u8,
    envelope_start: bool,
    envelope_divider: u8,
    envelope_decay: u8,
    shift_register: u16,
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
    fn write_control(&mut self, value: u8) {
        self.length_halt = (value & 0x20) != 0;
        self.constant_volume = (value & 0x10) != 0;
        self.envelope_period = value & 0x0F;
    }

    fn write_period(&mut self, value: u8) {
        const NOISE_PERIODS: [u16; 16] = [
            4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068,
        ];
        self.mode = (value & 0x80) != 0;
        self.timer_reload = NOISE_PERIODS[(value & 0x0F) as usize];
    }

    fn write_length(&mut self, value: u8, length_enabled: bool) {
        if length_enabled {
            self.length_counter = LENGTH_TABLE[(value >> 3) as usize];
        }
        self.envelope_start = true;
    }

    fn tick_timer(&mut self) {
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

    fn quarter_frame_tick(&mut self) {
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

    fn half_frame_tick(&mut self) {
        if !self.length_halt && self.length_counter > 0 {
            self.length_counter -= 1;
        }
    }

    fn set_enabled(&mut self, enabled: bool) {
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

    fn output(&self) -> u8 {
        if !self.enabled || self.length_counter == 0 || (self.shift_register & 0x01) != 0 {
            return 0;
        }
        self.envelope_volume()
    }
}

#[derive(Clone, Copy)]
struct DmcState {
    enabled: bool,
    irq_enabled: bool,
    loop_flag: bool,
    irq_flag: bool,
    output_level: u8,
    sample_address: u16,
    sample_length: u16,
    current_address: u16,
    bytes_remaining: u16,
    sample_buffer: Option<u8>,
    shift_register: u8,
    bits_remaining: u8,
    silence: bool,
    rate_index: u8,
    timer_reload: u16,
    timer_counter: u16,
}

impl Default for DmcState {
    fn default() -> Self {
        Self {
            enabled: false,
            irq_enabled: false,
            loop_flag: false,
            irq_flag: false,
            output_level: 0,
            sample_address: 0,
            sample_length: 0,
            current_address: 0,
            bytes_remaining: 0,
            sample_buffer: None,
            shift_register: 0,
            bits_remaining: 8,
            silence: true,
            rate_index: 0,
            timer_reload: 0,
            timer_counter: 0,
        }
    }
}

impl DmcState {
    fn write_control(&mut self, value: u8) {
        const DMC_PERIODS_NTSC: [u16; 16] = [
            428, 380, 340, 320, 286, 254, 226, 214, 190, 160, 142, 128, 106, 84, 72, 54,
        ];
        self.irq_enabled = (value & 0x80) != 0;
        self.loop_flag = (value & 0x40) != 0;
        self.rate_index = value & 0x0F;
        self.timer_reload = DMC_PERIODS_NTSC[self.rate_index as usize];
        if !self.irq_enabled {
            self.irq_flag = false;
        }
    }

    fn write_output_level(&mut self, value: u8) {
        self.output_level = value & 0x7F;
    }

    fn write_sample_address(&mut self, value: u8) {
        self.sample_address = 0xC000 | ((value as u16) << 6);
    }

    fn write_sample_length(&mut self, value: u8) {
        self.sample_length = ((value as u16) << 4) | 0x0001;
    }

    fn set_enabled(&mut self, enabled: bool) -> Option<DmcDmaRequest> {
        self.enabled = enabled;
        if !enabled {
            self.bytes_remaining = 0;
            self.sample_buffer = None;
            self.silence = true;
            return None;
        }

        if self.bytes_remaining == 0 {
            self.current_address = self.sample_address;
            self.bytes_remaining = self.sample_length;
            self.silence = true;
            return Some(DmcDmaRequest {
                addr: self.current_address,
                kind: DmcDmaKind::Load,
            });
        }

        None
    }

    fn request_dma_if_needed(&self) -> Option<DmcDmaRequest> {
        if self.sample_buffer.is_none() && self.bytes_remaining > 0 {
            Some(DmcDmaRequest {
                addr: self.current_address,
                kind: DmcDmaKind::Load,
            })
        } else {
            None
        }
    }

    fn submit_dma_sample(&mut self, value: u8) {
        self.sample_buffer = Some(value);
        self.current_address = self.current_address.wrapping_add(1);
        if self.current_address < 0x8000 {
            self.current_address = 0x8000;
        }
        if self.bytes_remaining > 0 {
            self.bytes_remaining -= 1;
            if self.bytes_remaining == 0 {
                if self.loop_flag {
                    self.current_address = self.sample_address;
                    self.bytes_remaining = self.sample_length;
                } else if self.irq_enabled {
                    self.irq_flag = true;
                }
            }
        }
    }

    fn tick_timer(&mut self) {
        if self.timer_counter == 0 {
            self.timer_counter = self.timer_reload;
            if !self.silence {
                if (self.shift_register & 1) != 0 {
                    if self.output_level <= 125 {
                        self.output_level += 2;
                    }
                } else if self.output_level >= 2 {
                    self.output_level -= 2;
                }
                self.shift_register >>= 1;
            }

            if self.bits_remaining > 0 {
                self.bits_remaining -= 1;
            }

            if self.bits_remaining == 0 {
                self.bits_remaining = 8;
                if let Some(next) = self.sample_buffer.take() {
                    self.shift_register = next;
                    self.silence = false;
                } else {
                    self.silence = true;
                }
            }
        } else {
            self.timer_counter -= 1;
        }
    }
}

pub struct APU {
    pulse1: PulseChannel,
    pulse2: PulseChannel,
    triangle: TriangleChannel,
    noise: NoiseChannel,
    dmc: DmcState,
    frame_irq_inhibit: bool,
    frame_irq_flag: bool,
    frame_mode_five_step: bool,
    frame_step: u8,
    frame_divider: u16,
    pending_dmc_dma: Option<DmcDmaRequest>,
    sample_rate: u32,
    cycles_per_sample: f64,
    sample_phase: f64,
    sample_accum: f64,
    sample_accum_count: u32,
    audio_samples: Vec<f32>,
    expansions: Vec<Box<dyn ExpansionAudioChip>>,
    apu_subclock_even: bool,
    pulse1_phase: f32,
    pulse2_phase: f32,
    dc_killer: f32,
    lpf_accum: f32,
    debug_mute_mask: u8,
}

impl APU {
    pub fn new() -> Self {
        let sample_rate = DEFAULT_SAMPLE_RATE;
        Self {
            pulse1: PulseChannel::new(true),
            pulse2: PulseChannel::new(false),
            triangle: TriangleChannel::default(),
            noise: NoiseChannel::default(),
            dmc: DmcState::default(),
            frame_irq_inhibit: false,
            frame_irq_flag: false,
            frame_mode_five_step: false,
            frame_step: 0,
            frame_divider: FRAME_SEQUENCER_DIVIDER,
            pending_dmc_dma: None,
            sample_rate,
            cycles_per_sample: CPU_CLOCK_NTSC / sample_rate as f64,
            sample_phase: 0.0,
            sample_accum: 0.0,
            sample_accum_count: 0,
            audio_samples: Vec::new(),
            expansions: Vec::new(),
            apu_subclock_even: false,
            pulse1_phase: 0.0,
            pulse2_phase: 0.0,
            dc_killer: 0.0,
            lpf_accum: 0.0,
            debug_mute_mask: 0,
        }
    }

    pub fn add_expansion_chip(&mut self, chip: Box<dyn ExpansionAudioChip>) {
        self.expansions.push(chip);
    }

    pub fn reset(&mut self) {
        self.frame_irq_flag = false;
        self.frame_step = 0;
        self.frame_divider = FRAME_SEQUENCER_DIVIDER;
        self.pending_dmc_dma = None;
        self.sample_phase = 0.0;
        self.sample_accum = 0.0;
        self.sample_accum_count = 0;
        self.apu_subclock_even = false;
        self.pulse1_phase = 0.0;
        self.pulse2_phase = 0.0;
        self.dc_killer = 0.0;
        self.lpf_accum = 0.0;
    }

    pub fn tick_cpu_cycle(&mut self) {
        self.tick_frame_counter();
        if self.apu_subclock_even {
            self.pulse1.tick_timer();
            self.pulse2.tick_timer();
            self.noise.tick_timer();
        }
        self.apu_subclock_even = !self.apu_subclock_even;
        self.triangle.tick_timer();
        self.dmc.tick_timer();
        if self.pending_dmc_dma.is_none() {
            self.pending_dmc_dma = self.dmc.request_dma_if_needed();
        }
        for chip in &mut self.expansions {
            chip.tick_cpu_cycle();
        }

        let raw_cycle_mix = self.mix_nonpulse_output();
        self.sample_accum += raw_cycle_mix as f64;
        self.sample_accum_count = self.sample_accum_count.saturating_add(1);

        self.sample_phase += 1.0;
        if self.sample_phase >= self.cycles_per_sample {
            self.sample_phase -= self.cycles_per_sample;
            let nonpulse = if self.sample_accum_count > 0 {
                (self.sample_accum / self.sample_accum_count as f64) as f32
            } else {
                raw_cycle_mix
            };
            let pulse = self.sample_blep_pulses();
            self.sample_accum = 0.0;
            self.sample_accum_count = 0;
            let filtered = self.filter_output((pulse + nonpulse).clamp(-1.0, 1.0));
            self.audio_samples.push(filtered);
        }
    }

    pub fn read_status_at_offset(&mut self, _cycle_offset: u8) -> u8 {
        let mut status = 0u8;
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
        if self.dmc.bytes_remaining > 0 {
            status |= 0x10;
        }
        if self.frame_irq_flag {
            status |= 0x40;
        }
        if self.dmc.irq_flag {
            status |= 0x80;
        }
        self.frame_irq_flag = false;
        status
    }

    pub fn write_register_at_offset(&mut self, addr: u16, data: u8, _cycle_offset: u8) {
        match addr {
            0x4000 => self.pulse1.write_control(data),
            0x4001 => self.pulse1.write_sweep(data),
            0x4002 => self.pulse1.write_timer_low(data),
            0x4003 => {
                self.pulse1.write_timer_high(data, self.pulse1.enabled);
                self.pulse1_phase = 0.0;
            }
            0x4004 => self.pulse2.write_control(data),
            0x4005 => self.pulse2.write_sweep(data),
            0x4006 => self.pulse2.write_timer_low(data),
            0x4007 => {
                self.pulse2.write_timer_high(data, self.pulse2.enabled);
                self.pulse2_phase = 0.0;
            }
            0x4008 => self.triangle.write_linear_control(data),
            0x400A => self.triangle.write_timer_low(data),
            0x400B => self.triangle.write_timer_high(data, self.triangle.enabled),
            0x400C => self.noise.write_control(data),
            0x400E => self.noise.write_period(data),
            0x400F => self.noise.write_length(data, self.noise.enabled),
            0x4010 => self.dmc.write_control(data),
            0x4011 => self.dmc.write_output_level(data),
            0x4012 => self.dmc.write_sample_address(data),
            0x4013 => self.dmc.write_sample_length(data),
            0x4015 => self.write_status(data),
            0x4017 => self.write_frame_counter(data),
            _ => {
                for chip in &mut self.expansions {
                    chip.cpu_write(addr, data);
                }
            }
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        if sample_rate == 0 || sample_rate == self.sample_rate {
            return;
        }

        self.sample_rate = sample_rate;
        self.cycles_per_sample = CPU_CLOCK_NTSC / sample_rate as f64;
        self.sample_phase = 0.0;
        self.sample_accum = 0.0;
        self.sample_accum_count = 0;
        self.audio_samples.clear();
        self.pulse1_phase = 0.0;
        self.pulse2_phase = 0.0;
        self.dc_killer = 0.0;
        self.lpf_accum = 0.0;
    }

    pub fn audio_samples(&self) -> &[f32] {
        &self.audio_samples
    }

    pub fn clear_audio_samples(&mut self) {
        self.audio_samples.clear();
    }

    pub fn irq_line(&self) -> bool {
        self.frame_irq_flag
            || self.dmc.irq_flag
            || self.expansions.iter().any(|chip| chip.irq_line())
    }

    pub fn set_debug_mute_mask(&mut self, mask: u8) {
        self.debug_mute_mask = mask & 0x1F;
    }

    pub fn debug_mute_mask(&self) -> u8 {
        self.debug_mute_mask
    }

    pub fn take_dmc_dma_request(&mut self) -> Option<DmcDmaRequest> {
        self.pending_dmc_dma.take()
    }

    pub fn submit_dmc_dma_sample(&mut self, data: u8) {
        self.dmc.submit_dma_sample(data);
    }

    pub fn save_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.pulse1.enabled as u8);
        writer.write_u8(self.pulse1.length_counter);
        writer.write_u8(self.pulse2.enabled as u8);
        writer.write_u8(self.pulse2.length_counter);
        writer.write_u8(self.triangle.enabled as u8);
        writer.write_u8(self.triangle.length_counter);
        writer.write_u8(self.noise.enabled as u8);
        writer.write_u8(self.noise.length_counter);
        writer.write_bool(self.frame_irq_inhibit);
        writer.write_bool(self.frame_irq_flag);
        writer.write_bool(self.frame_mode_five_step);
        writer.write_u8(self.frame_step);
        writer.write_u16(self.frame_divider);
        writer.write_bool(self.dmc.enabled);
        writer.write_bool(self.dmc.irq_enabled);
        writer.write_bool(self.dmc.loop_flag);
        writer.write_bool(self.dmc.irq_flag);
        writer.write_u8(self.dmc.output_level);
        writer.write_u16(self.dmc.sample_address);
        writer.write_u16(self.dmc.sample_length);
        writer.write_u16(self.dmc.current_address);
        writer.write_u16(self.dmc.bytes_remaining);
        writer.write_bool(self.dmc.silence);
        writer.write_bool(self.apu_subclock_even);
    }

    pub fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        self.pulse1.enabled = reader.read_u8()? != 0;
        self.pulse1.length_counter = reader.read_u8()?;
        self.pulse2.enabled = reader.read_u8()? != 0;
        self.pulse2.length_counter = reader.read_u8()?;
        self.triangle.enabled = reader.read_u8()? != 0;
        self.triangle.length_counter = reader.read_u8()?;
        self.noise.enabled = reader.read_u8()? != 0;
        self.noise.length_counter = reader.read_u8()?;
        self.frame_irq_inhibit = reader.read_bool()?;
        self.frame_irq_flag = reader.read_bool()?;
        self.frame_mode_five_step = reader.read_bool()?;
        self.frame_step = reader.read_u8()?;
        self.frame_divider = reader.read_u16()?;
        self.dmc.enabled = reader.read_bool()?;
        self.dmc.irq_enabled = reader.read_bool()?;
        self.dmc.loop_flag = reader.read_bool()?;
        self.dmc.irq_flag = reader.read_bool()?;
        self.dmc.output_level = reader.read_u8()?;
        self.dmc.sample_address = reader.read_u16()?;
        self.dmc.sample_length = reader.read_u16()?;
        self.dmc.current_address = reader.read_u16()?;
        self.dmc.bytes_remaining = reader.read_u16()?;
        self.dmc.silence = reader.read_bool()?;
        self.apu_subclock_even = reader.read_bool()?;
        self.pending_dmc_dma = None;
        self.sample_accum = 0.0;
        self.sample_accum_count = 0;
        self.pulse1_phase = 0.0;
        self.pulse2_phase = 0.0;
        self.dc_killer = 0.0;
        self.lpf_accum = 0.0;
        Ok(())
    }

    fn write_status(&mut self, data: u8) {
        self.pulse1.set_enabled((data & 0x01) != 0);
        self.pulse2.set_enabled((data & 0x02) != 0);
        self.triangle.set_enabled((data & 0x04) != 0);
        self.noise.set_enabled((data & 0x08) != 0);
        self.pending_dmc_dma = self.dmc.set_enabled((data & 0x10) != 0);
        self.dmc.irq_flag = false;
    }

    fn write_frame_counter(&mut self, data: u8) {
        self.frame_mode_five_step = (data & 0x80) != 0;
        self.frame_irq_inhibit = (data & 0x40) != 0;
        if self.frame_irq_inhibit {
            self.frame_irq_flag = false;
        }
        self.frame_step = 0;
        self.frame_divider = FRAME_SEQUENCER_DIVIDER + 8;

        if self.frame_mode_five_step {
            self.clock_quarter_frame();
            self.clock_half_frame();
        }
    }

    fn tick_frame_counter(&mut self) {
        if self.frame_divider == 0 {
            self.frame_divider = FRAME_SEQUENCER_DIVIDER;
            self.clock_frame_step();
        } else {
            self.frame_divider -= 1;
        }
    }

    fn clock_frame_step(&mut self) {
        if !self.frame_mode_five_step {
            self.clock_quarter_frame();
            if self.frame_step == 1 || self.frame_step == 3 {
                self.clock_half_frame();
            }
            if self.frame_step == 3 && !self.frame_irq_inhibit {
                self.frame_irq_flag = true;
            }
            self.frame_step = (self.frame_step + 1) & 0x03;
            return;
        }

        if self.frame_step != 3 {
            self.clock_quarter_frame();
        }
        if self.frame_step == 1 || self.frame_step == 4 {
            self.clock_half_frame();
        }
        self.frame_step = (self.frame_step + 1) % 5;
    }

    fn clock_quarter_frame(&mut self) {
        self.pulse1.quarter_frame_tick();
        self.pulse2.quarter_frame_tick();
        self.triangle.quarter_frame_tick();
        self.noise.quarter_frame_tick();
    }

    fn clock_half_frame(&mut self) {
        self.pulse1.half_frame_tick();
        self.pulse2.half_frame_tick();
        self.triangle.half_frame_tick();
        self.noise.half_frame_tick();
    }

    fn mix_nonpulse_output(&self) -> f32 {
        let triangle = if (self.debug_mute_mask & 0x04) == 0 {
            self.triangle.output() as f64
        } else {
            0.0
        };
        let noise = if (self.debug_mute_mask & 0x08) == 0 {
            self.noise.output() as f64
        } else {
            0.0
        };
        let dmc = if (self.debug_mute_mask & 0x10) == 0 {
            self.dmc.output_level as f64
        } else {
            0.0
        };

        let tnd_input = (triangle / 8227.0) + (noise / 12241.0) + (dmc / 22638.0);
        let tnd_mix = if tnd_input > 0.0 {
            159.79 / ((1.0 / tnd_input) + 100.0)
        } else {
            0.0
        };

        let mut mixed = tnd_mix;
        for chip in &self.expansions {
            mixed += chip.output_sample() as f64;
        }

        (mixed as f32).clamp(-1.0, 1.0)
    }

    fn sample_blep_pulses(&mut self) -> f32 {
        let pulse1 = if (self.debug_mute_mask & 0x01) == 0 {
            sample_blep_pulse_channel(self.pulse1, &mut self.pulse1_phase, self.sample_rate)
        } else {
            0.0
        };
        let pulse2 = if (self.debug_mute_mask & 0x02) == 0 {
            sample_blep_pulse_channel(self.pulse2, &mut self.pulse2_phase, self.sample_rate)
        } else {
            0.0
        };
        let pulse_sum = f64::from(pulse1 + pulse2);
        if pulse_sum > 0.0 {
            (95.88 / ((8128.0 / pulse_sum) + 100.0)) as f32
        } else {
            0.0
        }
    }

    fn filter_output(&mut self, input: f32) -> f32 {
        // Match the reference chain in refs/nesjs:
        // DC-killer high-pass + strong one-pole low-pass.
        let mut sample = input - self.dc_killer;
        self.dc_killer += sample * (1.0 / 256.0);
        self.dc_killer += if sample >= 0.0 {
            1.0 / 32768.0
        } else {
            -1.0 / 32768.0
        };
        sample = sample.clamp(-1.0, 1.0);

        self.lpf_accum += 0.5 * (sample - self.lpf_accum);
        self.lpf_accum.clamp(-1.0, 1.0)
    }
}

impl Default for APU {
    fn default() -> Self {
        Self::new()
    }
}

fn sample_blep_pulse_channel(channel: PulseChannel, phase: &mut f32, sample_rate: u32) -> f32 {
    if !channel.enabled
        || channel.length_counter == 0
        || channel.timer_reload < 8
        || channel.timer_reload > 0x07FF
        || channel.sweep_mute
        || sample_rate == 0
    {
        return 0.0;
    }

    let volume = channel.envelope_volume() as f32;
    if volume <= 0.0 {
        return 0.0;
    }

    let duty = match channel.duty & 0x03 {
        0 => 0.125,
        1 => 0.25,
        2 => 0.5,
        _ => 0.75,
    };
    let freq = CPU_CLOCK_NTSC_F32 / (16.0 * (channel.timer_reload as f32 + 1.0));
    let mut dt = freq / sample_rate as f32;
    dt = dt.clamp(1.0 / (sample_rate as f32 * 8.0), 0.49);

    let mut v = if *phase < duty { 1.0 } else { 0.0 };
    v += poly_blep(*phase, dt);
    let mut tf = *phase - duty;
    if tf < 0.0 {
        tf += 1.0;
    }
    v -= poly_blep(tf, dt);

    *phase += dt;
    if *phase >= 1.0 {
        *phase -= 1.0;
    }

    (v * volume).clamp(0.0, 15.0)
}

fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests;
