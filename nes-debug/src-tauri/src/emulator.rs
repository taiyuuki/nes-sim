use nes_sim::{Breakpoint, FrontendInput, FrontendRuntime, RunMode};
use std::cell::RefCell;

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
    pub pixels: Vec<u8>,
}

#[tauri::command]
pub fn load_rom(path: String) -> Result<(), String> {
    let rom = std::fs::read(&path).map_err(|e| format!("读取 ROM 失败: {e}"))?;
    let runtime = FrontendRuntime::from_rom_bytes(&rom)
        .map_err(|e| format!("加载 ROM 失败: {e}"))?;
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
pub fn run_frame() -> Result<DebugInfo, String> {
    with_runtime(|rt| {
        rt.nes_mut().set_paused(false);
        let input = FrontendInput::default();
        let snap = rt.step(input);
        Ok(debug_info_from_snapshot(&snap))
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
            pixels: video.pixels.to_vec(),
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

#[tauri::command]
pub fn read_oam() -> Result<Vec<u8>, String> {
    with_runtime(|rt| Ok(rt.nes().debug_memory_snapshot().oam.to_vec()))
}

#[tauri::command]
pub fn read_palette() -> Result<Vec<u8>, String> {
    with_runtime(|rt| Ok(rt.nes().debug_memory_snapshot().palette.to_vec()))
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

fn debug_info_from_snapshot(
    snap: &nes_sim::RuntimeSnapshot,
) -> DebugInfo {
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
