//! nes-sim 核心库的 WebAssembly 绑定。
//!
//! 面向 JS 前端的精简 API:加载 ROM、逐帧运行、取 RGBA 帧缓冲与音频采样、
//! 手柄输入、存档/读档、电池 RAM。
//!
//! 手柄位掩码(`set_controller_bits`):
//! A=1, B=2, Select=4, Start=8, Up=16, Down=32, Left=64, Right=128。

use nes_sim::video::{palette_index_to_rgb, palette_index_to_rgb_tinted};
use nes_sim::{ControllerState, FRAME_HEIGHT, FRAME_WIDTH, NES};
use wasm_bindgen::prelude::*;

fn button_bits(button: &str) -> Option<u8> {
    match button {
        "A" => Some(0x01),
        "B" => Some(0x02),
        "Select" => Some(0x04),
        "Start" => Some(0x08),
        "Up" => Some(0x10),
        "Down" => Some(0x20),
        "Left" => Some(0x40),
        "Right" => Some(0x80),
        _ => None,
    }
}

const RGBA_LEN: usize = FRAME_WIDTH * FRAME_HEIGHT * 4;

#[wasm_bindgen]
pub struct NesWasm {
    nes: NES,
    /// 复用的 RGBA 帧缓冲,避免每帧分配。
    rgba: Vec<u8>,
}

#[wasm_bindgen]
impl NesWasm {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            nes: NES::new(),
            rgba: vec![0; RGBA_LEN],
        }
    }

    /// 按格式嗅探加载 iNES ROM(FDS 请用 `load_fds`)。
    pub fn load_rom(&mut self, rom: &[u8]) -> Result<(), JsError> {
        self.nes.load_rom(rom, None).map_err(js_err)
    }

    /// 加载 FDS 磁盘镜像(需提供 BIOS ROM 数据)。
    pub fn load_fds(&mut self, image: &[u8], bios: &[u8]) -> Result<(), JsError> {
        self.nes.load_rom(image, Some(bios)).map_err(js_err)
    }

    /// 近似硬件 RESET 键。
    pub fn reset(&mut self) {
        self.nes.reset();
    }

    /// 模拟断电重启(CPU RAM 清零后复位)。
    pub fn power_cycle(&mut self) {
        self.nes.power_cycle();
    }

    /// 运行到下一帧完成,返回新的帧号。
    pub fn run_frame(&mut self) -> u64 {
        self.nes.run_frame();
        self.nes.frame_number()
    }

    /// 帧号。
    pub fn frame_number(&mut self) -> u64 {
        self.nes.frame_number()
    }

    /// 主时钟周期数。
    pub fn master_clock(&self) -> u64 {
        self.nes.master_clock()
    }

    /// 8 位调色板索引帧(256×240),适合配合 `palette_rgb` 使用。
    pub fn frame_pixels(&self) -> Vec<u8> {
        self.nes.frame_pixels().to_vec()
    }

    /// RGBA 帧缓冲(256×240×4),可直接写入 `ImageData`。
    /// 已应用当前帧的颜色强调($2001 bit5-7)。
    pub fn frame_rgba(&mut self) -> Vec<u8> {
        let emphasis = self.nes.video_frame().emphasis;
        let pixels = self.nes.frame_pixels();
        for (dst, &index) in self.rgba.as_chunks_mut::<4>().0.iter_mut().zip(pixels) {
            let [r, g, b] = palette_index_to_rgb_tinted(index, emphasis);
            dst[0] = r;
            dst[1] = g;
            dst[2] = b;
            dst[3] = 255;
        }
        self.rgba.clone()
    }

    /// 当前帧的颜色强调掩码(bit0=R, bit1=G, bit2=B)。
    pub fn emphasis_bits(&self) -> u8 {
        self.nes.video_frame().emphasis
    }

    /// 系统调色板,64 色 × 3 字节 RGB(不含强调)。
    pub fn palette_rgb() -> Vec<u8> {
        (0..64)
            .flat_map(|i| palette_index_to_rgb(i as u8))
            .collect()
    }

    /// 应用颜色强调后的系统调色板,64 色 × 3 字节 RGB
    /// (emphasis 为 `emphasis_bits` 返回的掩码)。
    pub fn palette_rgb_tinted(emphasis: u8) -> Vec<u8> {
        (0..64)
            .flat_map(|i| palette_index_to_rgb_tinted(i as u8, emphasis))
            .collect()
    }

    /// 自上次读取以来累积的音频采样(单声道 f32)。
    pub fn audio_samples(&self) -> Vec<f32> {
        self.nes.apu_audio_samples().to_vec()
    }

    /// 清空累积的音频采样(每轮播放后调用)。
    pub fn clear_audio_samples(&mut self) {
        self.nes.clear_apu_audio_samples();
    }

    /// 当前音频采样率(Hz)。
    pub fn audio_sample_rate(&self) -> u32 {
        self.nes.apu_sample_rate()
    }

    /// 设置 APU 采样率(对齐前端 AudioContext,如 48000)。
    pub fn set_audio_sample_rate(&mut self, rate: u32) {
        self.nes.set_apu_sample_rate(rate);
    }

    /// 设置手柄位掩码:A=1 B=2 Select=4 Start=8 Up=16 Down=32 Left=64 Right=128。
    pub fn set_controller_bits(&mut self, port: u8, bits: u8) {
        self.nes
            .set_controller_state(port as usize, ControllerState::from_bits(bits));
    }

    /// 便捷方法:按名称按键(其余按键状态保持不变)。
    /// 名称为 "A" "B" "Select" "Start" "Up" "Down" "Left" "Right"。
    pub fn set_button(&mut self, port: u8, button: &str, pressed: bool) {
        let Some(bits) = button_bits(button) else {
            return;
        };
        let port = port as usize;
        let merged = {
            let old = self.nes.controller_state(port).bits();
            if pressed { old | bits } else { old & !bits }
        };
        self.nes
            .set_controller_state(port, ControllerState::from_bits(merged));
    }

    /// 当前手柄位掩码。
    pub fn controller_bits(&self, port: u8) -> u8 {
        self.nes.controller_state(port as usize).bits()
    }

    /// 当前卡带 PRG‖CHR 的 MD5 十六进制字符串。
    pub fn rom_md5_hex(&self) -> Option<String> {
        self.nes.rom_md5_hex()
    }

    /// 存档(快照)。
    pub fn save_state(&self) -> Result<Vec<u8>, JsError> {
        self.nes.save_state().map_err(js_err)
    }

    /// 读档。
    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), JsError> {
        self.nes.load_state(bytes).map_err(js_err)
    }

    /// 电池后备 PRG RAM(无电池卡带返回 null)。
    pub fn battery_ram(&self) -> Option<Vec<u8>> {
        self.nes.battery_ram().map(|ram| ram.to_vec())
    }

    /// 载入电池后备 PRG RAM(.sav 文件内容)。
    pub fn load_battery_ram(&mut self, data: &[u8]) {
        self.nes.load_battery_ram(data);
    }

    /// FDS 磁盘是否被写入过(写回 .sav 用);非 FDS 返回 false。
    pub fn fds_dirty(&self) -> bool {
        self.nes.fds_dirty()
    }

    /// FDS 每面磁盘数据扁平拼接(配合 `fds_side_lengths` 切分);非 FDS 返回 null。
    pub fn fds_sides_flattened(&self) -> Option<Vec<u8>> {
        self.nes.fds_sides().map(|sides| sides.concat())
    }

    /// FDS 每面磁盘数据的字节长度(切分 `fds_sides_flattened` 用)。
    pub fn fds_side_lengths(&self) -> Vec<u32> {
        self.nes
            .fds_sides()
            .map(|sides| sides.iter().map(|s| s.len() as u32).collect())
            .unwrap_or_default()
    }
}

impl Default for NesWasm {
    fn default() -> Self {
        Self::new()
    }
}

fn js_err<E: std::fmt::Display>(err: E) -> JsError {
    JsError::new(&err.to_string())
}
