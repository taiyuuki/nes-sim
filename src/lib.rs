mod apu;
mod bus;
pub mod cartridge;
mod cpu;
mod dma;
mod input;
mod ppu;

pub use cartridge::{Cartridge, CartridgeError, Mirroring};
pub use input::{ControllerButton, ControllerState};
pub use ppu::{FRAME_HEIGHT, FRAME_WIDTH};
use std::fs;
use std::io;
use std::path::Path;

pub struct NES {
    pub cpu: cpu::CPU,
    pub bus: bus::NESBus,
    master_clock: u64,
    cpu_ppu_counter: u8,
    cpu_schedule_index: usize,
}

impl NES {
    pub fn new() -> Self {
        Self {
            cpu: cpu::CPU::new(),
            bus: bus::NESBus::new(),
            master_clock: 0,
            cpu_ppu_counter: 0,
            cpu_schedule_index: 0,
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
        self.master_clock += 1;
        self.bus.tick_ppu();
        self.cpu.set_nmi(self.bus.ppu_nmi_line());

        let cpu_schedule = self.bus.ppu().cpu_schedule();
        self.cpu_ppu_counter += 1;
        if self.cpu_ppu_counter >= cpu_schedule[self.cpu_schedule_index] {
            self.cpu_ppu_counter = 0;
            self.cpu_schedule_index = (self.cpu_schedule_index + 1) % cpu_schedule.len();
            self.bus.tick_apu_cpu_cycle();
            self.cpu.clock(&mut self.bus);
            self.cpu.irq_set_level(0x01, self.bus.apu_irq_line());
            self.cpu.set_nmi(self.bus.ppu_nmi_line());
            self.bus.advance_dma_cpu_phase();
        }
    }

    pub fn run_frame(&mut self) {
        let start_frame = self.bus.ppu_frame();
        while self.bus.ppu_frame() == start_frame {
            self.clock();
        }
    }

    pub fn master_clock(&self) -> u64 {
        self.master_clock
    }

    pub fn frame_pixels(&self) -> &[u8] {
        self.bus.ppu().frame_pixels()
    }

    pub fn frame_rgb(&self) -> Vec<u8> {
        self.bus.ppu().frame_rgb()
    }

    pub fn frame_ppm(&self) -> Vec<u8> {
        let rgb = self.frame_rgb();
        let mut ppm = Vec::with_capacity(16 + rgb.len());
        ppm.extend_from_slice(format!("P6\n{} {}\n255\n", FRAME_WIDTH, FRAME_HEIGHT).as_bytes());
        ppm.extend_from_slice(&rgb);
        ppm
    }

    pub fn write_frame_ppm<P: AsRef<Path>>(&self, path: P) -> io::Result<()> {
        fs::write(path, self.frame_ppm())
    }

    fn reset_cpu_schedule(&mut self) {
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
