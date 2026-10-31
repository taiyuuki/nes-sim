use crate::cartridge::TVSystem;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

pub(crate) mod dmc;
pub(crate) mod noise;
pub(crate) mod pulse;
pub(crate) mod triangle;

use dmc::{DmcDmaRequest, DmcState};
use noise::NoiseChannel;
use pulse::PulseChannel;
use triangle::TriangleChannel;

pub(crate) const CPU_CLOCK_NTSC: u64 = 1_789_773;
pub(crate) const CPU_CLOCK_PAL: u64 = 1_662_607;
pub(crate) const CPU_CLOCK_DENDY: u64 = 1_773_447;
const DEFAULT_SAMPLE_RATE: u32 = 44_100;

/// 帧序列器位置计数模型：事件在帧内固定位置触发（CPU周期，相对$4017复位生效点）。
/// PAL/Dendy 的帧计数器为 PAL 型（Nesdev APU Frame Counter；Dendy 克隆芯片即 2A07 语义）。
struct FrameSequencerTiming {
    period_4step: u16,
    period_5step: u16,
    quarter_steps: [u16; 4],
    /// 半帧（长度计数器时钟）位置：第二半帧在帧回绕拍(pos=0)
    half_steps: [u16; 2],
}

const FRAME_TIMING_NTSC: FrameSequencerTiming = FrameSequencerTiming {
    period_4step: 29_830,
    period_5step: 37_282,
    quarter_steps: [7_457, 14_913, 22_371, 29_829],
    half_steps: [14_914, 0],
};

const FRAME_TIMING_PAL: FrameSequencerTiming = FrameSequencerTiming {
    period_4step: 33_254,
    period_5step: 41_566,
    quarter_steps: [8_313, 16_627, 24_939, 33_253],
    half_steps: [16_628, 0],
};

/// 4步模式末尾IRQ标志持续置位的拍数（自quarter_steps[3]起，读清除后仍会重新置位）
const FRAME_IRQ_SET_TICKS: u8 = 3;

// 使用 Q31.31 定点数格式进行相位跟踪
// 高 32 位是整数部分，低 32 位是小数部分
const PHASE_SCALE: u64 = 1u64 << 32;

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
    fn clock_quarter_frame(&mut self) {}
    fn clock_half_frame(&mut self) {}
    fn irq_line(&self) -> bool {
        false
    }
    fn output_sample(&self) -> f32 {
        0.0
    }
    /// 复位芯片状态（NSF切曲用；普通卡带不走这条路径）。
    fn reset(&mut self) {}
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
    frame_pos: u16,
    frame_irq_set_ticks: u8,
    frame_reset_delay: u8,
    pending_dmc_dma: Option<DmcDmaRequest>,
    sample_rate: u32,
    // 当前制式的CPU主频（采样率换算用）与帧序列器位置表
    cpu_clock: u64,
    frame_timing: &'static FrameSequencerTiming,
    // 使用 Q31.31 定点数：每个 CPU 周期增加 PHASE_SCALE / cycles_per_sample
    cycles_per_sample_fixed: u64,
    sample_phase_fixed: u64,
    // 整数累加器，避免每 CPU 周期的浮点运算
    sample_accum_pulse: i64,
    sample_accum_tri: i64,
    sample_accum_noise: i64,
    sample_accum_dmc: i64,
    sample_accum_exp: i64,
    sample_accum_count: u32,
    audio_samples: Vec<f32>,
    expansions: Vec<Box<dyn ExpansionAudioChip>>,
    apu_subclock_even: bool,
    has_expansions: bool,
    debug_mute_mask: u8,
    /// 上电起的CPU周期绝对计数（寄存器访问亚周期定位用）
    apu_cycle: u64,
    /// $4015写事件队列：在写入的真实周期(offset)生效（Nestopia式）
    pending_4015_writes: Vec<Pending4015Write>,
}

/// $4015写延迟生效事件
#[derive(Clone, Copy)]
struct Pending4015Write {
    data: u8,
    due_cycle: u64,
}

impl APU {
    pub fn new() -> Self {
        let sample_rate = DEFAULT_SAMPLE_RATE;
        // 使用定点数：cycles_per_sample_fixed = (PHASE_SCALE * sample_rate) / cpu_clock
        // 这样 sample_phase_fixed 每次增加 cycles_per_sample_fixed
        let cycles_per_sample_fixed = (PHASE_SCALE * sample_rate as u64) / CPU_CLOCK_NTSC;
        Self {
            pulse1: PulseChannel::new(true),
            pulse2: PulseChannel::new(false),
            triangle: TriangleChannel::default(),
            noise: NoiseChannel::default(),
            dmc: DmcState::default(),
            frame_irq_inhibit: false,
            frame_irq_flag: false,
            frame_mode_five_step: false,
            frame_pos: 0,
            frame_irq_set_ticks: 0,
            frame_reset_delay: 0,
            pending_dmc_dma: None,
            sample_rate,
            cpu_clock: CPU_CLOCK_NTSC,
            frame_timing: &FRAME_TIMING_NTSC,
            cycles_per_sample_fixed,
            sample_phase_fixed: 0,
            sample_accum_pulse: 0,
            sample_accum_tri: 0,
            sample_accum_noise: 0,
            sample_accum_dmc: 0,
            sample_accum_exp: 0,
            sample_accum_count: 0,
            audio_samples: Vec::new(),
            expansions: Vec::new(),
            apu_subclock_even: false,
            has_expansions: false,
            debug_mute_mask: 0,
            apu_cycle: 0,
            pending_4015_writes: Vec::new(),
        }
    }

    pub fn add_expansion_chip(&mut self, chip: Box<dyn ExpansionAudioChip>) {
        self.has_expansions = true;
        self.expansions.push(chip);
    }

    /// 换卡时清空上一张卡的扩展芯片。
    pub fn clear_expansion_chips(&mut self) {
        self.expansions.clear();
        self.has_expansions = false;
    }

    /// 复位全部扩展芯片（NSF切曲用）。
    pub fn reset_expansion_chips(&mut self) {
        for chip in &mut self.expansions {
            chip.reset();
        }
    }

    /// 上电起累计的CPU周期数（含NSF挂起期间；播放计时用）。
    pub(crate) fn cycle_count(&self) -> u64 {
        self.apu_cycle
    }

    pub fn set_tv_system(&mut self, tv: TVSystem) {
        self.dmc.set_tv_system(tv);
        self.noise.set_tv_system(tv);
        // Dendy 的帧计数器与 noise/DMC 周期表均为 PAL 型（2A07 克隆芯片），
        // 仅 CPU 分频不同（主时钟/15 而非 /16）
        self.frame_timing = match tv {
            TVSystem::NTSC => &FRAME_TIMING_NTSC,
            TVSystem::PAL | TVSystem::DENDY => &FRAME_TIMING_PAL,
        };
        self.cpu_clock = match tv {
            TVSystem::NTSC => CPU_CLOCK_NTSC,
            TVSystem::PAL => CPU_CLOCK_PAL,
            TVSystem::DENDY => CPU_CLOCK_DENDY,
        };
        self.update_sample_phase();
    }

    pub fn reset(&mut self) {
        self.frame_irq_flag = false;
        self.frame_pos = 0;
        self.frame_irq_set_ticks = 0;
        self.frame_reset_delay = 0;
        self.pending_dmc_dma = None;
        self.sample_phase_fixed = 0;
        self.sample_accum_pulse = 0;
        self.sample_accum_tri = 0;
        self.sample_accum_noise = 0;
        self.sample_accum_dmc = 0;
        self.sample_accum_exp = 0;
        self.sample_accum_count = 0;
        self.apu_subclock_even = false;
    }

    pub fn tick_cpu_cycle(&mut self) {
        self.apu_cycle += 1;
        self.apply_due_4015_writes(self.apu_cycle);
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
            self.pending_dmc_dma = self.dmc.take_fetch_request();
        }
        if self.has_expansions {
            for chip in &mut self.expansions {
                chip.tick_cpu_cycle();
            }
        }

        // Fast path: skip mute mask checks when no channels are muted (common case)
        let mask = self.debug_mute_mask;
        let (p1, p2, tri, noise, dmc) = if mask == 0 {
            (
                self.pulse1.output(),
                self.pulse2.output(),
                self.triangle.output(),
                self.noise.output(),
                self.dmc.output_level,
            )
        } else {
            (
                if mask & 0x01 != 0 {
                    0
                } else {
                    self.pulse1.output()
                },
                if mask & 0x02 != 0 {
                    0
                } else {
                    self.pulse2.output()
                },
                if mask & 0x04 != 0 {
                    0
                } else {
                    self.triangle.output()
                },
                if mask & 0x08 != 0 {
                    0
                } else {
                    self.noise.output()
                },
                if mask & 0x10 != 0 {
                    0
                } else {
                    self.dmc.output_level
                },
            )
        };

        // 使用整数累加器，避免每 CPU 周期的浮点运算
        self.sample_accum_pulse += i64::from(p1 + p2);
        self.sample_accum_tri += i64::from(tri);
        self.sample_accum_noise += i64::from(noise);
        self.sample_accum_dmc += i64::from(dmc);

        if !self.expansions.is_empty() {
            let mut exp_out = 0i64;
            for chip in &self.expansions {
                exp_out += (chip.output_sample() * 1000.0) as i64;
            }
            self.sample_accum_exp += exp_out;
        }

        self.sample_accum_count = self.sample_accum_count.saturating_add(1);

        self.sample_phase_fixed += self.cycles_per_sample_fixed;
        if self.sample_phase_fixed >= PHASE_SCALE {
            self.sample_phase_fixed -= PHASE_SCALE;
            let count = self.sample_accum_count.max(1);
            // 只在输出时转换为浮点，使用乘法代替除法
            let count_f64 = count as f64;
            let inv_count = 1.0 / count_f64;
            let avg_pulse = self.sample_accum_pulse as f64 * inv_count;
            let avg_tri = self.sample_accum_tri as f64 * inv_count;
            let avg_noise = self.sample_accum_noise as f64 * inv_count;
            let avg_dmc = self.sample_accum_dmc as f64 * inv_count;
            let avg_exp = self.sample_accum_exp as f64 * inv_count / 1000.0;

            // 使用原始浮点混合公式
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

            self.sample_accum_pulse = 0;
            self.sample_accum_tri = 0;
            self.sample_accum_noise = 0;
            self.sample_accum_dmc = 0;
            self.sample_accum_exp = 0;
            self.sample_accum_count = 0;

            self.audio_samples.push(sample);
        }
    }

    pub fn read_status_at_offset(&mut self, cycle_offset: u8) -> u8 {
        // 读发生在指令起点后offset拍：先消费该时刻前到期的写事件
        self.apply_due_4015_writes(self.apu_cycle + u64::from(cycle_offset));
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

    pub fn write_register_at_offset(&mut self, addr: u16, data: u8, cycle_offset: u8) {
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
            0x4015 => {
                // 写在指令起点后offset拍生效（exe_inst在指令首拍执行全部访问）;
                // offset=0(无亚周期信息的直调/单测)立即应用
                if cycle_offset == 0 {
                    self.write_status(data);
                } else {
                    let due = self.apu_cycle + u64::from(cycle_offset);
                    self.pending_4015_writes.push(Pending4015Write {
                        data,
                        due_cycle: due,
                    });
                }
            }
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
        self.update_sample_phase();
        self.sample_phase_fixed = 0;
        self.sample_accum_pulse = 0;
        self.sample_accum_tri = 0;
        self.sample_accum_noise = 0;
        self.sample_accum_dmc = 0;
        self.sample_accum_exp = 0;
        self.sample_accum_count = 0;
        self.audio_samples.clear();
    }

    /// 按当前制式的CPU主频重算每个CPU周期对应的采样相位增量。
    fn update_sample_phase(&mut self) {
        self.cycles_per_sample_fixed = (PHASE_SCALE * u64::from(self.sample_rate)) / self.cpu_clock;
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
        writer.write_u16(self.frame_pos);
        writer.write_u8(self.frame_irq_set_ticks);
        writer.write_u8(self.frame_reset_delay);
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
        writer.write_bool(self.dmc.fetch_pending);
        writer.write_bool(self.apu_subclock_even);
        writer.write_bool(self.has_expansions);
        writer.write_u64(self.apu_cycle);
        writer.write_u32(self.pending_4015_writes.len() as u32);
        for write in &self.pending_4015_writes {
            writer.write_u8(write.data);
            writer.write_u64(write.due_cycle);
        }
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
        self.frame_pos = reader.read_u16()?;
        self.frame_irq_set_ticks = reader.read_u8()?;
        self.frame_reset_delay = reader.read_u8()?;
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
        self.dmc.fetch_pending = reader.read_bool()?;
        self.apu_subclock_even = reader.read_bool()?;
        self.has_expansions = reader.read_bool()?;
        self.apu_cycle = reader.read_u64()?;
        let queue_len = reader.read_u32()? as usize;
        self.pending_4015_writes.clear();
        for _ in 0..queue_len {
            let data = reader.read_u8()?;
            let due_cycle = reader.read_u64()?;
            self.pending_4015_writes
                .push(Pending4015Write { data, due_cycle });
        }
        self.pending_dmc_dma = None;
        self.sample_accum_pulse = 0;
        self.sample_accum_tri = 0;
        self.sample_accum_noise = 0;
        self.sample_accum_dmc = 0;
        self.sample_accum_exp = 0;
        self.sample_accum_count = 0;
        Ok(())
    }

    /// 消费due_cycle <= until的排队写事件
    fn apply_due_4015_writes(&mut self, until: u64) {
        while !self.pending_4015_writes.is_empty() && self.pending_4015_writes[0].due_cycle <= until
        {
            let write = self.pending_4015_writes.remove(0);
            self.write_status(write.data);
        }
    }

    fn write_status(&mut self, data: u8) {
        self.pulse1.set_enabled((data & 0x01) != 0);
        self.pulse2.set_enabled((data & 0x02) != 0);
        self.triangle.set_enabled((data & 0x04) != 0);
        self.noise.set_enabled((data & 0x08) != 0);
        self.dmc.set_enabled((data & 0x10) != 0);
        self.dmc.irq_flag = false;
    }

    fn write_frame_counter(&mut self, data: u8) {
        self.frame_mode_five_step = (data & 0x80) != 0;
        self.frame_irq_inhibit = (data & 0x40) != 0;
        if self.frame_irq_inhibit {
            self.frame_irq_flag = false;
        }
        // 帧计数器复位在写入后3~4个CPU周期才生效（按APU半周期奇偶对齐）
        self.frame_reset_delay = if self.apu_subclock_even { 2 } else { 3 };
    }

    fn tick_frame_counter(&mut self) {
        if self.frame_reset_delay > 0 {
            self.frame_reset_delay -= 1;
            if self.frame_reset_delay == 0 {
                self.frame_pos = 0;
                if self.frame_mode_five_step {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                }
            }
            return;
        }

        self.frame_pos = self.frame_pos.wrapping_add(1);
        let timing = self.frame_timing;
        let period = if self.frame_mode_five_step {
            timing.period_5step
        } else {
            timing.period_4step
        };
        if self.frame_pos >= period {
            self.frame_pos = 0;
        }

        // IRQ置位窗口：读清除后仍会重新置位，直到窗口结束
        if self.frame_irq_set_ticks > 0 {
            self.frame_irq_set_ticks -= 1;
            if !self.frame_irq_inhibit {
                self.frame_irq_flag = true;
            }
        }

        if timing.quarter_steps.contains(&self.frame_pos) {
            self.clock_quarter_frame();
        }
        if timing.half_steps.contains(&self.frame_pos) {
            self.clock_half_frame();
        }
        if !self.frame_mode_five_step
            && self.frame_pos == timing.quarter_steps[3]
            && !self.frame_irq_inhibit
        {
            self.frame_irq_flag = true;
            self.frame_irq_set_ticks = FRAME_IRQ_SET_TICKS - 1;
        }
    }

    fn clock_quarter_frame(&mut self) {
        self.pulse1.quarter_frame_tick();
        self.pulse2.quarter_frame_tick();
        self.triangle.quarter_frame_tick();
        self.noise.quarter_frame_tick();
        for chip in &mut self.expansions {
            chip.clock_quarter_frame();
        }
    }

    fn clock_half_frame(&mut self) {
        self.pulse1.half_frame_tick();
        self.pulse2.half_frame_tick();
        self.triangle.half_frame_tick();
        self.noise.half_frame_tick();
        for chip in &mut self.expansions {
            chip.clock_half_frame();
        }
    }
}

impl Default for APU {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
