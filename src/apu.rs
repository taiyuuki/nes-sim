use crate::savestate::{SaveStateError, StateReader, StateWriter};

pub(crate) mod dmc;
pub(crate) mod noise;
pub(crate) mod pulse;
pub(crate) mod triangle;

use dmc::{DmcDmaRequest, DmcState};
use noise::NoiseChannel;
use pulse::PulseChannel;
use triangle::TriangleChannel;

const CPU_CLOCK_NTSC: f64 = 1_789_773.0;
const DEFAULT_SAMPLE_RATE: u32 = 44_100;
const FRAME_SEQUENCER_DIVIDER: u16 = 7_456;
pub const LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

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
    sample_accum_pulse: f64,
    sample_accum_tri: f64,
    sample_accum_noise: f64,
    sample_accum_dmc: f64,
    sample_accum_exp: f64,
    sample_accum_count: u32,
    audio_samples: Vec<f32>,
    expansions: Vec<Box<dyn ExpansionAudioChip>>,
    apu_subclock_even: bool,
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
            sample_accum_pulse: 0.0,
            sample_accum_tri: 0.0,
            sample_accum_noise: 0.0,
            sample_accum_dmc: 0.0,
            sample_accum_exp: 0.0,
            sample_accum_count: 0,
            audio_samples: Vec::new(),
            expansions: Vec::new(),
            apu_subclock_even: false,
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
        self.sample_accum_pulse = 0.0;
        self.sample_accum_tri = 0.0;
        self.sample_accum_noise = 0.0;
        self.sample_accum_dmc = 0.0;
        self.sample_accum_exp = 0.0;
        self.sample_accum_count = 0;
        self.apu_subclock_even = false;
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

        let p1 = if (self.debug_mute_mask & 0x01) != 0 {
            0
        } else {
            self.pulse1.output()
        };
        let p2 = if (self.debug_mute_mask & 0x02) != 0 {
            0
        } else {
            self.pulse2.output()
        };
        let tri = if (self.debug_mute_mask & 0x04) != 0 {
            0
        } else {
            self.triangle.output()
        };
        let noise = if (self.debug_mute_mask & 0x08) != 0 {
            0
        } else {
            self.noise.output()
        };
        let dmc = if (self.debug_mute_mask & 0x10) != 0 {
            0
        } else {
            self.dmc.output_level
        };

        self.sample_accum_pulse += f64::from(p1 + p2);
        self.sample_accum_tri += f64::from(tri);
        self.sample_accum_noise += f64::from(noise);
        self.sample_accum_dmc += f64::from(dmc);

        let mut exp_out = 0.0f64;
        for chip in &self.expansions {
            exp_out += chip.output_sample() as f64;
        }
        self.sample_accum_exp += exp_out;

        self.sample_accum_count = self.sample_accum_count.saturating_add(1);

        self.sample_phase += 1.0;
        if self.sample_phase >= self.cycles_per_sample {
            self.sample_phase -= self.cycles_per_sample;
            let count = self.sample_accum_count.max(1) as f64;
            let avg_pulse = self.sample_accum_pulse / count;
            let avg_tri = self.sample_accum_tri / count;
            let avg_noise = self.sample_accum_noise / count;
            let avg_dmc = self.sample_accum_dmc / count;
            let avg_exp = self.sample_accum_exp / count;

            let pulse_mix = if avg_pulse > 0.0 {
                (95.88 / ((8128.0 / avg_pulse) + 100.0)) as f32
            } else {
                0.0
            };

            let tnd_input = avg_tri / 8227.0 + avg_noise / 12241.0 + avg_dmc / 22638.0;
            let tnd_mix = if tnd_input > 0.0 {
                159.79 / ((1.0 / tnd_input) + 100.0)
            } else {
                0.0
            };

            let sample = (pulse_mix + tnd_mix as f32 + avg_exp as f32).clamp(-1.0, 1.0);

            self.sample_accum_pulse = 0.0;
            self.sample_accum_tri = 0.0;
            self.sample_accum_noise = 0.0;
            self.sample_accum_dmc = 0.0;
            self.sample_accum_exp = 0.0;
            self.sample_accum_count = 0;

            self.audio_samples.push(sample);
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
            0x4003 => self.pulse1.write_timer_high(data, self.pulse1.enabled),
            0x4004 => self.pulse2.write_control(data),
            0x4005 => self.pulse2.write_sweep(data),
            0x4006 => self.pulse2.write_timer_low(data),
            0x4007 => self.pulse2.write_timer_high(data, self.pulse2.enabled),
            0x4008 => self.triangle.write_linear_control(data),
            0x400A => self.triangle.write_timer_low(data),
            0x400B => {
                self.triangle.write_timer_high(data, self.triangle.enabled);
            }
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
        self.sample_accum_pulse = 0.0;
        self.sample_accum_tri = 0.0;
        self.sample_accum_noise = 0.0;
        self.sample_accum_dmc = 0.0;
        self.sample_accum_exp = 0.0;
        self.sample_accum_count = 0;
        self.audio_samples.clear();
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
        self.sample_accum_pulse = 0.0;
        self.sample_accum_tri = 0.0;
        self.sample_accum_noise = 0.0;
        self.sample_accum_dmc = 0.0;
        self.sample_accum_exp = 0.0;
        self.sample_accum_count = 0;
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
}

impl Default for APU {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
