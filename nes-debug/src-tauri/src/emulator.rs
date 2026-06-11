use base64::{engine::general_purpose::STANDARD, Engine};
use nes_sim::{Breakpoint, FrontendInput, FrontendRuntime, Mirroring, RunMode};
use std::cell::RefCell;
use tauri::Emitter;

thread_local! {
    static RUNTIME: RefCell<Option<FrontendRuntime>> = const { RefCell::new(None) };
}

fn with_runtime<F, R>(f: F) -> Result<R, String>
where
    F: FnOnce(&mut FrontendRuntime) -> Result<R, String>,
{
    RUNTIME.with(|cell| {
        let mut guard = cell.borrow_mut();
        let runtime = guard.as_mut().ok_or("No ROM loaded")?;
        f(runtime)
    })
}

#[derive(serde::Serialize, Clone)]
pub struct CpuInfo {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub status: u8,
    pub clocks: u64,
    pub instruction_counter: u64,
    pub irq_pending: bool,
    pub nmi_line: bool,
}

#[derive(serde::Serialize, Clone)]
pub struct PpuInfo {
    pub frame: u64,
    pub scanline: i16,
    pub in_vblank: bool,
    pub nmi_line: bool,
    pub oam_addr: u8,
    pub cycles: u16,
    pub ctrl: u8,
    pub mask: u8,
    pub status: u8,
    pub vram_addr: u16,
    pub temp_vram_addr: u16,
    pub bg_on: bool,
    pub sprites_on: bool,
    pub rendering_on: bool,
}

#[derive(serde::Serialize, Clone)]
pub struct DebugInfo {
    pub master_clock: u64,
    pub cpu: CpuInfo,
    pub ppu: PpuInfo,
    pub paused: bool,
    pub frame_number: u64,
}

#[derive(serde::Serialize, Clone)]
pub struct FrameData {
    pub width: usize,
    pub height: usize,
    pub pixels_b64: String,
}

#[derive(serde::Serialize, Clone)]
pub struct RunFrameResult {
    pub debug_info: DebugInfo,
    pub frame: FrameData,
}

#[tauri::command]
pub fn load_rom(path: String) -> Result<(), String> {
    let rom = std::fs::read(&path).map_err(|e| format!("读取 ROM 失败: {e}"))?;
    let runtime =
        FrontendRuntime::from_rom_bytes(&rom).map_err(|e| format!("加载 ROM 失败: {e}"))?;
    RUNTIME.with(|cell| {
        *cell.borrow_mut() = Some(runtime);
    });
    Ok(())
}

#[tauri::command]
pub fn reset() -> Result<(), String> {
    with_runtime(|rt| {
        rt.nes_mut().reset();
        Ok(())
    })
}

#[tauri::command]
pub fn step_frame() -> Result<DebugInfo, String> {
    with_runtime(|rt| {
        rt.nes_mut().set_paused(false);
        rt.set_mode(RunMode::Paused);
        let input = FrontendInput {
            step_frame: true,
            ..Default::default()
        };
        let snap = rt.step(input);
        Ok(debug_info_from_snapshot(&snap))
    })
}

#[tauri::command]
pub fn step_instruction() -> Result<DebugInfo, String> {
    with_runtime(|rt| {
        rt.nes_mut().set_paused(false);
        rt.set_mode(RunMode::Paused);
        let input = FrontendInput {
            step_cpu_instruction: true,
            ..Default::default()
        };
        let snap = rt.step(input);
        Ok(debug_info_from_snapshot(&snap))
    })
}

#[tauri::command]
pub fn run_frame(controller: u8, window: tauri::Window) -> Result<RunFrameResult, String> {
    with_runtime(|rt| {
        rt.nes_mut().set_paused(false);
        let input = FrontendInput {
            controller1: nes_sim::ControllerState::from_bits(controller),
            ..Default::default()
        };

        let snap = rt.step(input);
        let debug_info = debug_info_from_snapshot(&snap);

        let video = snap.video;
        let frame = FrameData {
            width: video.width,
            height: video.height,
            pixels_b64: STANDARD.encode(video.pixels),
        };

        let audio_samples = rt.nes().apu_audio_samples();
        if !audio_samples.is_empty() {
            let audio_data = AudioData {
                samples: audio_samples.to_vec(),
                sample_rate: rt.nes().apu_sample_rate(),
            };
            let _ = window.emit("audio_data", audio_data);
            rt.nes_mut().clear_apu_audio_samples();
        }

        Ok(RunFrameResult { debug_info, frame })
    })
}

#[tauri::command]
pub fn toggle_pause() -> Result<DebugInfo, String> {
    with_runtime(|rt| {
        let input = FrontendInput {
            toggle_pause: true,
            ..Default::default()
        };
        let snap = rt.step(input);
        Ok(debug_info_from_snapshot(&snap))
    })
}

#[tauri::command]
pub fn get_debug_info() -> Result<DebugInfo, String> {
    with_runtime(|rt| {
        let snap = rt.snapshot();
        Ok(debug_info_from_snapshot(&snap))
    })
}

#[tauri::command]
pub fn get_frame() -> Result<FrameData, String> {
    with_runtime(|rt| {
        let video = rt.snapshot().video;
        Ok(FrameData {
            width: video.width,
            height: video.height,
            pixels_b64: STANDARD.encode(video.pixels),
        })
    })
}

#[tauri::command]
pub fn read_ram() -> Result<Vec<u8>, String> {
    with_runtime(|rt| Ok(rt.nes().debug_memory_snapshot().ram.to_vec()))
}

#[tauri::command]
pub fn read_vram() -> Result<Vec<u8>, String> {
    with_runtime(|rt| Ok(rt.nes().debug_memory_snapshot().vram.to_vec()))
}

#[tauri::command]
pub fn read_chr() -> Result<Vec<u8>, String> {
    with_runtime(|rt| Ok(rt.nes().debug_memory_snapshot().chr.to_vec()))
}

#[derive(serde::Serialize, Clone)]
pub struct PatternTableData {
    pub table0_b64: String,
    pub table1_b64: String,
    pub size: usize,
}

#[derive(serde::Serialize, Clone)]
pub struct NametableData {
    pub pixels_b64: String,
    pub width: usize,
    pub height: usize,
}

#[tauri::command]
pub fn get_pattern_tables() -> Result<PatternTableData, String> {
    with_runtime(|rt| {
        let chr = rt.nes_mut().debug_read_chr();

        let size = 128;
        let table0 = render_pattern_table(&chr, 0x0000);
        let table1 = render_pattern_table(&chr, 0x1000);

        Ok(PatternTableData {
            table0_b64: STANDARD.encode(&table0),
            table1_b64: STANDARD.encode(&table1),
            size,
        })
    })
}

#[tauri::command]
pub fn get_nametable(table_index: u8) -> Result<NametableData, String> {
    with_runtime(|rt| {
        let snap = rt.snapshot();
        let ctrl = snap.debug.ppu.ctrl;
        let mirroring = rt.nes().mirroring();
        let vram = rt.nes().debug_memory_snapshot().vram.to_vec();
        let chr = rt.nes_mut().debug_read_chr().to_vec();
        let palette = rt.nes().debug_memory_snapshot().palette.to_vec();

        let width = NT_WIDTH * TILE_SIZE;
        let height = NT_HEIGHT * TILE_SIZE;

        let pixels = render_nametable(&vram, &chr, &palette, table_index as usize, ctrl, mirroring);

        Ok(NametableData {
            pixels_b64: STANDARD.encode(&pixels),
            width,
            height,
        })
    })
}

fn render_pattern_table(chr: &[u8], offset: usize) -> Vec<u8> {
    let size = 128;
    let mut pixels = vec![0u8; size * size * 4];

    const COLORS: [[u8; 4]; 4] = [
        [24, 24, 24, 255],
        [96, 96, 96, 255],
        [180, 180, 180, 255],
        [255, 255, 255, 255],
    ];

    for tile_idx in 0..256 {
        let tile_row = tile_idx / 16;
        let tile_col = tile_idx % 16;
        let tile_addr = offset + tile_idx * 16;

        for y in 0..8 {
            let lo = chr.get(tile_addr + y).copied().unwrap_or(0);
            let hi = chr.get(tile_addr + 8 + y).copied().unwrap_or(0);

            for x in 0..8 {
                let bit_lo = (lo >> (7 - x)) & 1;
                let bit_hi = (hi >> (7 - x)) & 1;
                let color_idx = ((bit_hi << 1) | bit_lo) as usize;

                let px = tile_col * 8 + x;
                let py = tile_row * 8 + y;
                let idx = (py * size + px) * 4;

                let c = &COLORS[color_idx];
                pixels[idx] = c[0];
                pixels[idx + 1] = c[1];
                pixels[idx + 2] = c[2];
                pixels[idx + 3] = c[3];
            }
        }
    }
    pixels
}

const NT_WIDTH: usize = 32;
const NT_HEIGHT: usize = 30;
const TILE_SIZE: usize = 8;

fn render_nametable(
    vram: &[u8],
    chr: &[u8],
    palette: &[u8],
    table_idx: usize,
    ctrl: u8,
    mirroring: Mirroring,
) -> Vec<u8> {
    let width = NT_WIDTH * TILE_SIZE;
    let height = NT_HEIGHT * TILE_SIZE;
    let mut pixels = vec![0u8; width * height * 4];

    let bg_table = if (ctrl & 0x10) != 0 { 0x1000 } else { 0x0000 };

    // 根据 mirroring 模式计算 nametable 偏移
    // 模仿 ppu_memory.rs 中的 nametable_index 逻辑
    let nt_base_offset = match mirroring {
        Mirroring::Horizontal => {
            if table_idx == 0 || table_idx == 1 {
                0 // $2000/$2400 → VRAM[0-0x3FF]
            } else {
                0x0400 // $2800/$2C00 → VRAM[0x400-0x7FF]
            }
        }
        Mirroring::Vertical => {
            if table_idx == 0 || table_idx == 2 {
                0 // $2000/$2800 → VRAM[0-0x3FF]
            } else {
                0x0400 // $2400/$2C00 → VRAM[0x400-0x7FF]
            }
        }
        Mirroring::FourScreen => {
            table_idx * 0x0400 // 每个都有独立空间
        }
        Mirroring::SPAGE0 => {
            0 // 所有都映射到第一个页面
        }
        Mirroring::SPAGE1 => {
            0x0400 // 所有都映射到第二个页面
        }
    };

    for tile_y in 0..NT_HEIGHT {
        for tile_x in 0..NT_WIDTH {
            let tile_idx = tile_y * NT_WIDTH + tile_x;
            // nametable 数据在 VRAM 中的偏移
            let nt_addr = nt_base_offset + tile_idx;

            let tile_id = vram.get(nt_addr).copied().unwrap_or(0) as usize;
            let tile_addr = bg_table + tile_id * 16;

            // attribute table 在每个 nametable 后面
            let attr_block_x = tile_x / 4;
            let attr_block_y = tile_y / 4;
            let attr_shift = ((tile_y % 4) / 2) * 2 + ((tile_x % 4) / 2);
            let attr_byte_idx = attr_block_y * 8 + attr_block_x;
            let attr_addr = nt_base_offset + 0x03C0 + attr_byte_idx;
            let palette_idx =
                ((vram.get(attr_addr).copied().unwrap_or(0) >> attr_shift) & 0x03) as usize;

            let palette_base = [0, 4, 8, 12][palette_idx];
            let base_color = palette.get(0).copied().unwrap_or(0) as usize;
            let palette_colors = [
                base_color,
                palette.get(palette_base).copied().unwrap_or(0) as usize,
                palette.get(palette_base + 1).copied().unwrap_or(0) as usize,
                palette.get(palette_base + 2).copied().unwrap_or(0) as usize,
            ];

            for y in 0..8 {
                let lo = chr.get(tile_addr + y).copied().unwrap_or(0);
                let hi = chr.get(tile_addr + 8 + y).copied().unwrap_or(0);

                for x in 0..8 {
                    let bit_lo = (lo >> (7 - x)) & 1;
                    let bit_hi = (hi >> (7 - x)) & 1;
                    let pixel_idx = ((bit_hi << 1) | bit_lo) as usize;

                    let px = tile_x * TILE_SIZE + x;
                    let py = tile_y * TILE_SIZE + y;
                    let out_idx = (py * width + px) * 4;

                    let [r, g, b] =
                        nes_sim::video::palette_index_to_rgb(palette_colors[pixel_idx] as u8);
                    pixels[out_idx] = r;
                    pixels[out_idx + 1] = g;
                    pixels[out_idx + 2] = b;
                    pixels[out_idx + 3] = 255;
                }
            }
        }
    }
    pixels
}

#[tauri::command]
pub fn read_oam() -> Result<Vec<u8>, String> {
    with_runtime(|rt| Ok(rt.nes().debug_memory_snapshot().oam.to_vec()))
}

#[tauri::command]
pub fn read_palette() -> Result<Vec<u8>, String> {
    with_runtime(|rt| Ok(rt.nes().debug_memory_snapshot().palette.to_vec()))
}

#[derive(serde::Serialize, Clone)]
pub struct AudioData {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

#[tauri::command]
pub fn get_audio_samples() -> Result<AudioData, String> {
    with_runtime(|rt| {
        let samples = rt.nes().apu_audio_samples().to_vec();
        let sample_rate = rt.nes().apu_sample_rate();
        rt.nes_mut().clear_apu_audio_samples();
        println!(
            "获取音频样本: {} 个样本, 采样率: {}",
            samples.len(),
            sample_rate
        );
        Ok(AudioData {
            samples,
            sample_rate,
        })
    })
}

#[tauri::command]
pub fn set_audio_volume(percent: f64) -> Result<(), String> {
    with_runtime(|_rt| {
        // 将百分比转换为 APU 音量（0-255）
        let _volume = (percent / 100.0 * 255.0) as u8;
        // 设置为全局音量控制
        // 这里可以添加音量控制逻辑
        Ok(())
    })
}

#[derive(serde::Deserialize)]
pub struct BreakpointDef {
    #[serde(rename = "type")]
    pub bp_type: String,
    pub value: Option<u16>,
}

#[tauri::command]
pub fn add_breakpoint(bp_def: BreakpointDef) -> Result<(), String> {
    let bp = match bp_def.bp_type.as_str() {
        "address" => Breakpoint::Address(bp_def.value.unwrap_or(0)),
        "memory_read" => Breakpoint::MemoryRead(bp_def.value.unwrap_or(0)),
        "memory_write" => Breakpoint::MemoryWrite(bp_def.value.unwrap_or(0)),
        "ppu_scanline" => Breakpoint::PpuScanline(bp_def.value.unwrap_or(0) as i16),
        "vblank" => Breakpoint::Vblank,
        _ => return Err(format!("Unknown breakpoint type: {}", bp_def.bp_type)),
    };
    with_runtime(|rt| {
        rt.nes_mut().add_breakpoint(bp);
        Ok(())
    })
}

#[tauri::command]
pub fn remove_breakpoint(bp_def: BreakpointDef) -> Result<(), String> {
    let bp = match bp_def.bp_type.as_str() {
        "address" => Breakpoint::Address(bp_def.value.unwrap_or(0)),
        "memory_read" => Breakpoint::MemoryRead(bp_def.value.unwrap_or(0)),
        "memory_write" => Breakpoint::MemoryWrite(bp_def.value.unwrap_or(0)),
        "ppu_scanline" => Breakpoint::PpuScanline(bp_def.value.unwrap_or(0) as i16),
        "vblank" => Breakpoint::Vblank,
        _ => return Err(format!("Unknown breakpoint type: {}", bp_def.bp_type)),
    };
    with_runtime(|rt| {
        rt.nes_mut().remove_breakpoint(&bp);
        Ok(())
    })
}

#[tauri::command]
pub fn set_paused(paused: bool) -> Result<(), String> {
    with_runtime(|rt| {
        rt.nes_mut().set_paused(paused);
        if !paused {
            rt.set_mode(RunMode::Running);
        }
        Ok(())
    })
}

#[derive(serde::Serialize, Clone)]
pub struct DisasmInstruction {
    pub address: u16,
    pub bytes: [u8; 3],
    pub len: u8,
    pub mnemonic: String,
    pub operand: String,
}

#[derive(serde::Serialize, Clone)]
pub struct DisasmResult {
    pub instructions: Vec<DisasmInstruction>,
    pub pc_index: usize,
}

#[tauri::command]
pub fn disassemble(rows: usize) -> Result<DisasmResult, String> {
    with_runtime(|rt| {
        let result = rt.nes_mut().debug_disassemble(rows);
        Ok(DisasmResult {
            instructions: result
                .instructions
                .into_iter()
                .map(|i| DisasmInstruction {
                    address: i.address,
                    bytes: i.bytes,
                    len: i.len,
                    mnemonic: i.mnemonic,
                    operand: i.operand,
                })
                .collect(),
            pc_index: result.pc_index,
        })
    })
}

fn debug_info_from_snapshot(snap: &nes_sim::RuntimeSnapshot) -> DebugInfo {
    let d = &snap.debug;
    DebugInfo {
        master_clock: d.master_clock,
        cpu: CpuInfo {
            a: d.cpu.a,
            x: d.cpu.x,
            y: d.cpu.y,
            sp: d.cpu.sp,
            pc: d.cpu.pc,
            status: d.cpu.status,
            clocks: d.cpu.clocks,
            instruction_counter: d.cpu.instruction_counter,
            irq_pending: d.cpu.irq_pending,
            nmi_line: d.cpu.nmi_line,
        },
        ppu: PpuInfo {
            frame: d.ppu.frame,
            scanline: d.ppu.scanline,
            in_vblank: d.ppu.in_vblank,
            nmi_line: d.ppu.nmi_line,
            oam_addr: d.ppu.oam_addr,
            cycles: d.ppu.cycles,
            ctrl: d.ppu.ctrl,
            mask: d.ppu.mask,
            status: d.ppu.status,
            vram_addr: d.ppu.vram_addr,
            temp_vram_addr: d.ppu.temp_vram_addr,
            bg_on: d.ppu.bg_on,
            sprites_on: d.ppu.sprites_on,
            rendering_on: d.ppu.rendering_on,
        },
        paused: matches!(snap.status.mode, RunMode::Paused),
        frame_number: snap.video.frame_number,
    }
}
