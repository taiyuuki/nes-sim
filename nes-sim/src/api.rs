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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreCommand {
    Reset,
    SetControllerState { port: usize, state: ControllerState },
    RunFrame,
    StepCpuInstruction,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DebugSnapshot {
    pub master_clock: u64,
    pub cpu: CpuDebugSnapshot,
    pub ppu: PpuDebugSnapshot,
}
