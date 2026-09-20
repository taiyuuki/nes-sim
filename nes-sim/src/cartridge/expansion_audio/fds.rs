use std::cell::RefCell;
use std::rc::Rc;

use crate::apu::ExpansionAudioChip;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

// 调制表单位写入值（$4088 低3位）；索引4是特殊的"复位"标记，写作0x10
const BIAS_TABLE: [i32; 8] = [0, 1, 2, 4, 0, -4, -2, -1];
const MOD_RESET_MARKER: u8 = 0x10;

// 主时钟与CPU时钟之比为1:2，用2^32定点累加器每CPU周期推进0.5个主时钟
const MASTER_TICK_SCALE: u64 = 1 << 32;
const HALF_TICK: u64 = 1 << 31;

// 输出满幅：波表(63) × 音量(32) × 4 / 除数(2) = 4032
const OUTPUT_FULL_SCALE: f32 = 4032.0;
const OUTPUT_GAIN: f32 = 0.4;

/// FDS扩展音频通道：64项载波波表 + 32项调制表 + 两个包络。
/// 逻辑逐行移植自 fceux src/fds.cpp（2025年从Mednafen移植的调制/扫频版本）。
pub(crate) struct FdsAudio {
    mwave: [u8; 0x20],
    cwave: [u8; 0x40],
    amplitude: [u8; 2],
    // 每通道包络递减计数（fceux的counto，非持久化的静态变量，这里纳入状态）
    env_count: [i32; 2],
    // $4080-$408A 寄存器镜像（索引即地址-0x4080）
    regs: [u8; 0x0B],
    cwave_freq: u32,
    cwave_pos: u32,
    mod_freq: u32,
    mod_pos: u32,
    mod_disabled: bool,
    sweep_bias: u32,
    mod_out: i32,
    sample_cache: i32,
    // 主时钟累加器与包络周期计数（fceux的count/envcount）
    tick_accum: u64,
    env_timer: i32,
}

impl FdsAudio {
    pub(crate) fn new() -> Self {
        Self {
            mwave: [0; 0x20],
            cwave: [0; 0x40],
            amplitude: [0; 2],
            env_count: [0; 2],
            regs: [0; 0x0B],
            cwave_freq: 0,
            cwave_pos: 0,
            mod_freq: 0,
            mod_pos: 0,
            mod_disabled: false,
            sweep_bias: 0,
            mod_out: 0,
            sample_cache: 0,
            tick_accum: 0,
            env_timer: 0,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn reset(&mut self) {
        *self = Self::new();
    }

    /// $4040-$407F 波表读写；写仅在 $4089 bit7（波表写模式）置位时生效。
    pub(crate) fn read_wave(&self, index: usize) -> u8 {
        self.cwave[index & 0x3F] | 0x40
    }

    pub(crate) fn write_wave(&mut self, index: usize, data: u8) {
        if self.regs[0x9] & 0x80 != 0 {
            self.cwave[index & 0x3F] = data & 0x3F;
        }
    }

    /// $4090/$4092 增益读回。
    pub(crate) fn read_gain(&self, addr_low: u8) -> u8 {
        match addr_low & 0x0F {
            0x0 => self.amplitude[0] | 0x40,
            0x2 => self.amplitude[1] | 0x40,
            _ => 0x40,
        }
    }

    /// $4080-$408A 寄存器写入（index = 地址 - 0x4080）。
    pub(crate) fn write_reg(&mut self, index: usize, data: u8) {
        match index {
            0x0 | 0x4 => {
                // bit7=1 时包络禁用，直接写幅度
                if data & 0x80 != 0 {
                    self.amplitude[index >> 2] = data & 0x3F;
                }
            }
            0x2 => self.cwave_freq = (self.cwave_freq & 0xFF00) | u32::from(data),
            0x3 => {
                // 载波halt(bit7)释放时波表相位归零
                if data & 0x80 == 0 && self.regs[0x3] & 0x80 != 0 {
                    self.cwave_pos = 0;
                }
                self.cwave_freq = (self.cwave_freq & 0x00FF) | ((u32::from(data) & 0x0F) << 8);
            }
            0x5 => {
                self.sweep_bias = (u32::from(data) & 0x7F) << 4;
                self.mod_pos = 0;
            }
            0x6 => self.mod_freq = (self.mod_freq & 0xFF00) | u32::from(data),
            0x7 => {
                self.mod_freq = (self.mod_freq & 0x00FF) | ((u32::from(data) & 0x0F) << 8);
                self.mod_disabled = data & 0x80 != 0;
            }
            0x8
                // 调制表仅在halt($4087 bit7)期间可写：整体左移一格，末尾追加
                if self.mod_disabled => {
                    self.mwave.copy_within(1.., 0);
                    self.mwave[0x1F] = if data & 0x07 == 0x4 {
                        MOD_RESET_MARKER
                    } else {
                        BIAS_TABLE[(data & 0x07) as usize] as u8
                    };
                }
            _ => {}
        }
        self.regs[index] = data;
    }

    /// 每CPU周期推进（FDS主时钟为CPU的一半）。
    pub(crate) fn tick_cpu_cycle(&mut self) {
        // $4089 bit7 波表写模式期间通道halt（fceux在渲染循环外直接跳过推进）
        if self.regs[0x9] & 0x80 != 0 {
            return;
        }
        self.tick_accum += HALF_TICK;
        while self.tick_accum >= MASTER_TICK_SCALE {
            self.tick_accum -= MASTER_TICK_SCALE;
            self.master_tick();
        }
    }

    pub(crate) fn output_sample(&self) -> f32 {
        if self.regs[0x9] & 0x80 != 0 {
            return 0.0;
        }
        self.sample_cache as f32 / OUTPUT_FULL_SCALE * OUTPUT_GAIN
    }

    fn master_tick(&mut self) {
        let prev_cwave_pos = self.cwave_pos;
        self.clock_mod();
        // $4083 bit7 = 载波halt
        if self.regs[0x3] & 0x80 == 0 {
            self.clock_carrier();
        }
        self.env_timer -= 1;
        if self.env_timer <= 0 {
            self.env_timer += i32::from(self.regs[0x0A]) * 3;
            self.clock_envelopes();
        }
        // 波表相位越过一项时锁存输出电平
        if (self.cwave_pos ^ prev_cwave_pos) & (1 << 21) != 0 {
            let volume = i32::from(self.amplitude[0].min(0x20));
            let divisor = i32::from(self.regs[0x9] & 0x3) + 2;
            self.sample_cache =
                i32::from(self.cwave[(self.cwave_pos >> 21) as usize & 0x3F]) * volume * 4
                    / divisor;
        }
    }

    fn clock_mod(&mut self) {
        if self.mod_disabled {
            return;
        }
        let prev_mod_pos = self.mod_pos;
        self.mod_pos = self.mod_pos.wrapping_add(self.mod_freq);

        if (self.mod_pos & (0x3F << 11)) != (prev_mod_pos & (0x3F << 11)) {
            let mw = i32::from(self.mwave[(self.mod_pos >> 16) as usize & 0x1F]);
            self.sweep_bias = self.sweep_bias.wrapping_add(mw as u32) & 0x7FF;
            if mw == MOD_RESET_MARKER as i32 {
                self.sweep_bias = 0;
            }
        }

        // 11位有符号bias × 调制深度，带小数修正的除法（fceux注释：/16在Zelda里听感更好）
        let signed_bias = ((self.sweep_bias as i32) << 21) >> 21;
        let mut temp = signed_bias * i32::from(self.amplitude[1].min(0x20));
        if temp & 0x0F0 != 0 {
            temp /= 256;
            if self.sweep_bias & 0x400 != 0 {
                temp -= 1;
            } else {
                temp += 2;
            }
        } else {
            temp /= 256;
        }
        if temp >= 194 {
            temp -= 258;
        }
        if temp < -64 {
            temp += 256;
        }
        self.mod_out = temp;
    }

    fn clock_carrier(&mut self) {
        let freq = if self.mod_disabled {
            self.cwave_freq << 6
        } else {
            // 调制输出直接叠加到载波频率增量上
            let modulated = (self.cwave_freq << 6) as i32 + (self.cwave_freq as i32) * self.mod_out;
            modulated.max(0) as u32
        };
        self.cwave_pos = self.cwave_pos.wrapping_add(freq);
    }

    fn clock_envelopes(&mut self) {
        for channel in 0..2 {
            let ctrl = self.regs[channel << 2];
            // 包络仅在直写幅度(bit7)=0且总halt($4083 bit6)=0时运行
            if ctrl & 0x80 != 0 || self.regs[0x3] & 0x40 != 0 {
                continue;
            }
            if self.env_count[channel] <= 0 {
                if ctrl & 0x40 != 0 {
                    if self.amplitude[channel] < 0x3F {
                        self.amplitude[channel] += 1;
                    }
                } else if self.amplitude[channel] > 0 {
                    self.amplitude[channel] -= 1;
                }
                self.env_count[channel] = i32::from(ctrl & 0x3F);
            } else {
                self.env_count[channel] -= 1;
            }
        }
    }

    pub(crate) fn save_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(&self.mwave);
        writer.write_bytes(&self.cwave);
        writer.write_bytes(&self.amplitude);
        for &count in &self.env_count {
            writer.write_u32(count as u32);
        }
        writer.write_bytes(&self.regs);
        writer.write_u32(self.cwave_freq);
        writer.write_u32(self.cwave_pos);
        writer.write_u32(self.mod_freq);
        writer.write_u32(self.mod_pos);
        writer.write_bool(self.mod_disabled);
        writer.write_u32(self.sweep_bias);
        writer.write_u32(self.mod_out as u32);
        writer.write_u32(self.sample_cache as u32);
        writer.write_u64(self.tick_accum);
        writer.write_u32(self.env_timer as u32);
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.mwave)?;
        reader.read_bytes_into(&mut self.cwave)?;
        reader.read_bytes_into(&mut self.amplitude)?;
        for count in &mut self.env_count {
            *count = reader.read_u32()? as i32;
        }
        reader.read_bytes_into(&mut self.regs)?;
        self.cwave_freq = reader.read_u32()?;
        self.cwave_pos = reader.read_u32()?;
        self.mod_freq = reader.read_u32()?;
        self.mod_pos = reader.read_u32()?;
        self.mod_disabled = reader.read_bool()?;
        self.sweep_bias = reader.read_u32()?;
        self.mod_out = reader.read_u32()? as i32;
        self.sample_cache = reader.read_u32()? as i32;
        self.tick_accum = reader.read_u64()?;
        self.env_timer = reader.read_u32()? as i32;
        Ok(())
    }
}

pub(crate) struct FdsAudioChip {
    audio: Rc<RefCell<FdsAudio>>,
}

impl FdsAudioChip {
    pub(crate) fn new(audio: Rc<RefCell<FdsAudio>>) -> Self {
        Self { audio }
    }
}

impl ExpansionAudioChip for FdsAudioChip {
    fn tick_cpu_cycle(&mut self) {
        self.audio.borrow_mut().tick_cpu_cycle();
    }

    fn output_sample(&self) -> f32 {
        self.audio.borrow().output_sample()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fds_audio_produces_output_when_programmed() {
        let mut audio = FdsAudio::new();
        // 波表写模式：写入非零波形
        audio.write_reg(0x9, 0x80);
        for i in 0..0x40 {
            audio.write_wave(i, if i < 32 { 0x3F } else { 0x08 });
        }
        // 退出写模式（bit7=0），主音量档0（除数2）
        audio.write_reg(0x9, 0x00);
        // 直写幅度0x20
        audio.write_reg(0x0, 0x80 | 0x20);
        // 载波频率
        audio.write_reg(0x2, 0x00);
        audio.write_reg(0x3, 0x01);

        let mut peak = 0.0f32;
        let mut nonzero = false;
        for _ in 0..18000 {
            audio.tick_cpu_cycle();
            let sample = audio.output_sample();
            peak = peak.max(sample.abs());
            if sample != 0.0 {
                nonzero = true;
            }
        }
        assert!(nonzero, "FDS audio never produced a non-zero sample");
        assert!(peak > 0.05, "FDS audio peak too low: {peak}");
    }

    #[test]
    fn wave_write_blocked_outside_write_mode() {
        let mut audio = FdsAudio::new();
        audio.write_reg(0x9, 0x00);
        audio.write_wave(0, 0x3F);
        assert_eq!(audio.cwave[0], 0);
        audio.write_reg(0x9, 0x80);
        audio.write_wave(0, 0x3F);
        assert_eq!(audio.cwave[0], 0x3F);
    }

    #[test]
    fn mod_table_shift_write_and_reset_marker() {
        let mut audio = FdsAudio::new();
        // 调制halt($4087.7)时才可写$4088
        audio.write_reg(0x7, 0x80);
        audio.write_reg(0x8, 0x01); // +1
        audio.write_reg(0x8, 0x03); // +4
        audio.write_reg(0x8, 0x04); // reset marker 0x10
        assert_eq!(audio.mwave[0x1F], 0x10);
        assert_eq!(audio.mwave[0x1E], 4);
        assert_eq!(audio.mwave[0x1D], 1);
    }

    #[test]
    fn envelope_moves_amplitude_toward_target() {
        let mut audio = FdsAudio::new();
        // 递增包络: bit6=1, 速度1
        audio.write_reg(0x0, 0x40 | 0x01);
        audio.write_reg(0x3, 0x00);
        // 包络速度寄存器$408A=1 → 周期=3主时钟
        audio.write_reg(0xA, 0x01);
        for _ in 0..4 {
            audio.clock_envelopes();
        }
        assert!(audio.amplitude[0] > 0, "envelope did not raise amplitude");
    }
}
