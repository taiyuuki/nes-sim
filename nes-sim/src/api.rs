use crate::{ControllerState, FRAME_WIDTH};

pub const VIDEO_FRAME_PITCH: usize = FRAME_WIDTH;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Indexed8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VideoFrame<'a> {
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub format: PixelFormat,
    pub frame_number: u64,
    pub pixels: &'a [u8],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AudioBatch<'a> {
    pub channels: u8,
    pub sample_rate: u32,
    pub samples: &'a [f32],
}

impl<'a> Default for AudioBatch<'a> {
    fn default() -> Self {
        Self {
            channels: 1,
            sample_rate: 44_100,
            samples: &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoreCommand {
    Reset,
    SetControllerState {
        port: usize,
        state: ControllerState,
    },
    RunFrame,
    StepCpuInstruction,
    #[cfg(feature = "debug")]
    AddBreakpoint(Breakpoint),
    #[cfg(feature = "debug")]
    RemoveBreakpoint(Breakpoint),
    #[cfg(feature = "debug")]
    SetPaused(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreEvent {
    None,
    ResetComplete,
    ControllerStateUpdated { port: usize },
    FrameReady { frame_number: u64 },
    CpuInstructionComplete { instruction_counter: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoreResponse {
    pub event: CoreEvent,
    pub master_clock: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuDebugSnapshot {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub status: u8,
    pub clocks: u64,
    pub cycles_remaining: u64,
    pub instruction_counter: u64,
    pub irq_pending: bool,
    pub nmi_line: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PpuDebugSnapshot {
    pub frame: u64,
    pub scanline: i16,
    pub in_vblank: bool,
    pub nmi_line: bool,
    pub oam_addr: u8,
    #[cfg(feature = "debug")]
    pub cycles: u16,
    #[cfg(feature = "debug")]
    pub ctrl: u8,
    #[cfg(feature = "debug")]
    pub mask: u8,
    #[cfg(feature = "debug")]
    pub status: u8,
    #[cfg(feature = "debug")]
    pub fine_x: u8,
    #[cfg(feature = "debug")]
    pub vram_addr: u16,
    #[cfg(feature = "debug")]
    pub temp_vram_addr: u16,
    #[cfg(feature = "debug")]
    pub write_latch: bool,
    #[cfg(feature = "debug")]
    pub bg_on: bool,
    #[cfg(feature = "debug")]
    pub sprites_on: bool,
    #[cfg(feature = "debug")]
    pub rendering_on: bool,
    #[cfg(feature = "debug")]
    pub odd_frame: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebugSnapshot {
    pub master_clock: u64,
    pub cpu: CpuDebugSnapshot,
    pub ppu: PpuDebugSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemorySnapshot<'a> {
    pub ram: &'a [u8; 0x800],
    pub vram: &'a [u8; 0x1000],
    pub chr: &'a [u8; 0x2000],
    pub palette: &'a [u8; 0x20],
    pub oam: &'a [u8; 256],
}

#[cfg(feature = "debug")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breakpoint {
    Address(u16),
    MemoryRead(u16),
    MemoryWrite(u16),
    PpuScanline(i16),
    Vblank,
}

#[cfg(feature = "debug")]
#[derive(Debug, Clone, Default)]
pub struct Debugger {
    breakpoints: Vec<Breakpoint>,
    paused: bool,
}

#[cfg(feature = "debug")]
impl Debugger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_breakpoint(&mut self, bp: Breakpoint) {
        if !self.breakpoints.contains(&bp) {
            self.breakpoints.push(bp);
        }
    }

    pub fn remove_breakpoint(&mut self, bp: &Breakpoint) {
        self.breakpoints.retain(|b| b != bp);
    }

    pub fn clear_breakpoints(&mut self) {
        self.breakpoints.clear();
    }

    pub fn breakpoints(&self) -> &[Breakpoint] {
        &self.breakpoints
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn paused(&self) -> bool {
        self.paused
    }
}
