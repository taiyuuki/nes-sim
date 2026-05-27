pub mod api;
mod apu;
mod bus;
pub mod cartridge;
mod cpu;
mod dma;
pub mod headless;
mod input;
mod ppu;
mod ppu_memory;
pub mod runtime;
pub mod savestate;
pub mod video;

pub use api::{
    AudioBatch, CoreCommand, CoreEvent, CoreResponse, CpuDebugSnapshot, DebugSnapshot, PixelFormat,
    PpuDebugSnapshot, VIDEO_FRAME_PITCH, VideoFrame,
};
pub use apu::ExpansionAudioChip;
pub use cartridge::{Cartridge, CartridgeError, Mirroring, TVSystem};
pub use input::{ControllerButton, ControllerState};
pub use ppu::{FRAME_HEIGHT, FRAME_WIDTH};
pub use runtime::{
    ExecutionTarget, FrontendInput, FrontendRuntime, RunMode, RuntimeSnapshot, RuntimeStatus,
};
pub use savestate::SaveStateError;
use savestate::{StateReader, StateWriter};

const NTSC_CPU_SCHEDULE: [u8; 1] = [3];
const PAL_CPU_SCHEDULE: [u8; 5] = [3, 3, 3, 3, 4];
const DENDY_CPU_SCHEDULE: [u8; 1] = [3];

fn cpu_schedule(tv: TVSystem) -> &'static [u8] {
    match tv {
        TVSystem::NTSC => &NTSC_CPU_SCHEDULE,
        TVSystem::PAL => &PAL_CPU_SCHEDULE,
        TVSystem::DENDY => &DENDY_CPU_SCHEDULE,
    }
}

pub struct NES {
    cpu: cpu::CPU,
    bus: bus::NESBus,
    master_clock: u64,
    cpu_ppu_counter: u8,
    cpu_schedule_index: usize,
    cached_tv_system: TVSystem,
}

impl NES {
    pub fn new() -> Self {
        Self {
            cpu: cpu::CPU::new(),
            bus: bus::NESBus::new(),
            master_clock: 0,
            cpu_ppu_counter: 0,
            cpu_schedule_index: 0,
            cached_tv_system: TVSystem::NTSC,
        }
    }

    pub fn reset(&mut self) {
        self.bus.reset();
        self.reset_cpu_schedule();
        self.cpu.reset(&mut self.bus);
        self.cpu.set_nmi(self.bus.ppu_nmi_line());
    }

    pub fn insert_cartridge(&mut self, cartridge: Cartridge) {
        self.bus.insert_cartridge(cartridge);
        self.reset_cpu_schedule();
        self.reset();
    }

    pub fn load_cartridge_ines(&mut self, rom: &[u8]) -> Result<(), CartridgeError> {
        self.bus.load_cartridge_ines(rom)?;
        self.reset_cpu_schedule();
        Ok(())
    }

    pub fn set_controller_state(&mut self, port: usize, state: ControllerState) {
        self.bus.set_controller_state(port, state);
    }

    pub fn clock(&mut self) {
        self.bus.tick_ppu();

        let schedule = cpu_schedule(self.cached_tv_system);
        self.cpu_ppu_counter += 1;
        if self.cpu_ppu_counter >= schedule[self.cpu_schedule_index] {
            self.cpu_ppu_counter = 0;
            self.cpu_schedule_index = (self.cpu_schedule_index + 1) % schedule.len();
            // Update NMI/IRQ only around CPU step, not every PPU cycle
            self.cpu.set_nmi(self.bus.ppu_nmi_line());
            self.bus.advance_dma_cpu_phase();
            self.bus.tick_apu_cpu_cycle();
            self.cpu.clock(&mut self.bus);
            self.cpu.irq_set_level(0x01, self.bus.apu_irq_line());
            self.cpu.irq_set_level(0x02, self.bus.cartridge_irq_line());
            self.cpu.set_nmi(self.bus.ppu_nmi_line());
        }
    }

    pub fn run_frame(&mut self) {
        let start_frame = self.frame_number();
        while self.frame_number() == start_frame {
            self.clock();
        }
    }

    pub fn step_cpu_instruction(&mut self) {
        let start_instruction = self.cpu.instruction_counter();
        while self.cpu.instruction_counter() == start_instruction {
            self.clock();
        }
    }

    pub fn execute(&mut self, command: CoreCommand) -> CoreResponse {
        let event = match command {
            CoreCommand::Reset => {
                self.reset();
                CoreEvent::ResetComplete
            }
            CoreCommand::SetControllerState { port, state } => {
                self.set_controller_state(port, state);
                CoreEvent::ControllerStateUpdated { port }
            }
            CoreCommand::RunFrame => {
                self.run_frame();
                CoreEvent::FrameReady {
                    frame_number: self.frame_number(),
                }
            }
            CoreCommand::StepCpuInstruction => {
                self.step_cpu_instruction();
                CoreEvent::CpuInstructionComplete {
                    instruction_counter: self.cpu.instruction_counter(),
                }
            }
        };

        CoreResponse {
            event,
            master_clock: self.master_clock,
        }
    }

    pub fn master_clock(&self) -> u64 {
        self.master_clock
    }

    pub fn frame_number(&self) -> u64 {
        self.bus.ppu_frame()
    }

    pub fn frame_pixels(&self) -> &[u8] {
        self.bus.ppu().frame_pixels()
    }

    pub fn video_frame(&self) -> VideoFrame<'_> {
        VideoFrame {
            width: FRAME_WIDTH,
            height: FRAME_HEIGHT,
            pitch: VIDEO_FRAME_PITCH,
            format: PixelFormat::Indexed8,
            frame_number: self.frame_number(),
            pixels: self.frame_pixels(),
        }
    }

    pub fn audio_batch(&self) -> AudioBatch<'_> {
        AudioBatch {
            channels: 1,
            sample_rate: self.bus.apu_sample_rate(),
            samples: self.bus.apu_audio_samples(),
        }
    }

    pub fn add_expansion_audio_chip(&mut self, chip: Box<dyn ExpansionAudioChip>) {
        self.bus.add_expansion_audio_chip(chip);
    }

    pub fn debug_snapshot(&self) -> DebugSnapshot {
        let ppu = self.bus.ppu();
        DebugSnapshot {
            master_clock: self.master_clock,
            cpu: self.cpu.debug_snapshot(),
            ppu: PpuDebugSnapshot {
                frame: ppu.frame(),
                scanline: ppu.scanline(),
                in_vblank: ppu.in_vblank(),
                nmi_line: ppu.nmi_line(),
                oam_addr: ppu.oam_addr(),
            },
        }
    }

    pub fn save_state(&self) -> Result<Vec<u8>, SaveStateError> {
        let mut writer = StateWriter::new();
        writer.write_u64(self.master_clock);
        writer.write_u8(self.cpu_ppu_counter);
        writer.write_u64(self.cpu_schedule_index as u64);
        self.cpu.save_state(&mut writer);
        self.bus.save_state(&mut writer)?;
        Ok(writer.finish())
    }

    pub fn load_state(&mut self, bytes: &[u8]) -> Result<(), SaveStateError> {
        let mut reader = StateReader::new(bytes)?;
        self.master_clock = reader.read_u64()?;
        self.cpu_ppu_counter = reader.read_u8()?;
        self.cpu_schedule_index = reader.read_u64()? as usize;
        self.cpu.load_state(&mut reader)?;
        self.bus.load_state(&mut reader)?;
        self.cached_tv_system = self.bus.ppu().tv_system();
        self.cpu.set_nmi(self.bus.ppu_nmi_line());
        reader.finish()
    }

    pub fn clear_audio_samples(&mut self) {
        self.bus.clear_apu_audio_samples();
    }

    pub fn set_apu_sample_rate(&mut self, sample_rate: u32) {
        self.bus.set_apu_sample_rate(sample_rate);
    }

    pub fn set_apu_debug_mute_mask(&mut self, mask: u8) {
        self.bus.set_apu_debug_mute_mask(mask);
    }

    pub fn apu_debug_mute_mask(&self) -> u8 {
        self.bus.apu_debug_mute_mask()
    }

    fn reset_cpu_schedule(&mut self) {
        self.cached_tv_system = self.bus.ppu().tv_system();
        self.cpu_ppu_counter = 0;
        self.cpu_schedule_index = 0;
    }
}

impl Default for NES {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
