use crate::{
    AudioBatch, CartridgeError, ControllerState, CoreCommand, DebugSnapshot, NES, SaveStateError,
    VideoFrame,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrontendInput {
    pub controller1: ControllerState,
    pub controller2: ControllerState,
    pub reset: bool,
    pub toggle_pause: bool,
    pub step_frame: bool,
    pub step_cpu_instruction: bool,
    pub quit: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunMode {
    Running,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecutionTarget {
    None,
    Frame,
    CpuInstruction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuntimeStatus {
    pub mode: RunMode,
    pub executed: ExecutionTarget,
    pub quit_requested: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct RuntimeSnapshot<'a> {
    pub status: RuntimeStatus,
    pub video: VideoFrame<'a>,
    pub audio: AudioBatch<'a>,
    pub debug: DebugSnapshot,
}

pub struct FrontendRuntime {
    nes: NES,
    mode: RunMode,
}

impl FrontendRuntime {
    pub fn new(nes: NES) -> Self {
        Self {
            nes,
            mode: RunMode::Running,
        }
    }

    pub fn from_rom_bytes(rom: &[u8]) -> Result<Self, CartridgeError> {
        let mut nes = NES::new();
        nes.load_cartridge_ines(rom)?;
        nes.reset();
        Ok(Self::new(nes))
    }

    pub fn mode(&self) -> RunMode {
        self.mode
    }

    pub fn nes(&self) -> &NES {
        &self.nes
    }

    pub fn nes_mut(&mut self) -> &mut NES {
        &mut self.nes
    }

    pub fn save_state(&self) -> Result<Vec<u8>, SaveStateError> {
        self.nes.save_state()
    }

    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), SaveStateError> {
        self.nes.load_state(bytes)
    }

    pub fn set_mode(&mut self, mode: RunMode) {
        self.mode = mode;
    }

    pub fn snapshot(&self) -> RuntimeSnapshot<'_> {
        RuntimeSnapshot {
            status: RuntimeStatus {
                mode: self.mode,
                executed: ExecutionTarget::None,
                quit_requested: false,
            },
            video: self.nes.video_frame(),
            audio: self.nes.audio_batch(),
            debug: self.nes.debug_snapshot(),
        }
    }

    pub fn step(&mut self, input: FrontendInput) -> RuntimeSnapshot<'_> {
        self.nes.clear_audio_samples();
        self.nes.execute(CoreCommand::SetControllerState {
            port: 0,
            state: input.controller1,
        });
        self.nes.execute(CoreCommand::SetControllerState {
            port: 1,
            state: input.controller2,
        });

        if input.reset {
            self.nes.execute(CoreCommand::Reset);
        }

        if input.toggle_pause {
            self.mode = match self.mode {
                RunMode::Running => RunMode::Paused,
                RunMode::Paused => RunMode::Running,
            };
        }

        let executed = if input.step_cpu_instruction {
            self.nes.execute(CoreCommand::StepCpuInstruction);
            ExecutionTarget::CpuInstruction
        } else if input.step_frame || matches!(self.mode, RunMode::Running) {
            self.nes.execute(CoreCommand::RunFrame);
            ExecutionTarget::Frame
        } else {
            ExecutionTarget::None
        };

        RuntimeSnapshot {
            status: RuntimeStatus {
                mode: self.mode,
                executed,
                quit_requested: input.quit,
            },
            video: self.nes.video_frame(),
            audio: self.nes.audio_batch(),
            debug: self.nes.debug_snapshot(),
        }
    }
}
