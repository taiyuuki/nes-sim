mod bus;
pub mod cartridge;
mod cpu;
mod dma;
mod ppu;

pub use cartridge::{Cartridge, CartridgeError, Mirroring};

pub struct NES {
    pub cpu: cpu::CPU,
    pub bus: bus::NESBus,
    master_clock: u64,
}

impl NES {
    pub fn new() -> Self {
        Self {
            cpu: cpu::CPU::new(),
            bus: bus::NESBus::new(),
            master_clock: 0,
        }
    }

    pub fn reset(&mut self) {
        self.bus.reset();
        self.cpu.reset(&mut self.bus);
        self.cpu.set_nmi(self.bus.ppu_nmi_line());
    }

    pub fn insert_cartridge(&mut self, cartridge: Cartridge) {
        self.bus.insert_cartridge(cartridge);
    }

    pub fn load_cartridge_ines(&mut self, rom: &[u8]) -> Result<(), CartridgeError> {
        self.bus.load_cartridge_ines(rom)
    }

    pub fn clock(&mut self) {
        self.master_clock += 1;
        self.bus.tick_ppu();
        self.cpu.set_nmi(self.bus.ppu_nmi_line());

        if self.master_clock % 3 == 0 {
            self.cpu.clock(&mut self.bus);
            self.cpu.set_nmi(self.bus.ppu_nmi_line());
        }
    }

    pub fn master_clock(&self) -> u64 {
        self.master_clock
    }
}

impl Default for NES {
    fn default() -> Self {
        Self::new()
    }
}
