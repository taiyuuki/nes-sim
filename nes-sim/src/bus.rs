use crate::apu::APU;
use crate::cartridge::{Cartridge, CartridgeError};
use crate::dma::{DmaBusRequest, DmaController};
use crate::input::{ControllerState, Joypad};
use crate::ppu::PPU;
use crate::ppu_memory::PPUMemory;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

pub trait CPUBus {
    fn cpu_read(&mut self, addr: u16) -> u8;
    fn cpu_write(&mut self, addr: u16, data: u8);
    fn cpu_read_timed(&mut self, addr: u16, cycle_offset: u8) -> u8 {
        let _ = cycle_offset;
        self.cpu_read(addr)
    }
    fn cpu_write_timed(&mut self, addr: u16, data: u8, cycle_offset: u8) {
        let _ = cycle_offset;
        self.cpu_write(addr, data);
    }
    fn try_dma(&mut self) -> bool {
        false
    }

    fn cpu_read_u16(&mut self, addr: u16) -> u16 {
        let lo = self.cpu_read(addr) as u16;
        let hi = self.cpu_read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }
}

pub struct NESBus {
    pub ram: [u8; 0x800],
    ppu: PPU,
    ppu_memory: PPUMemory,
    apu: APU,
    dma: DmaController,
    controllers: [Joypad; 2],
    cpu_open_bus: u8,
    // Additional components: APU, cartridge, etc. can be added here
}

impl NESBus {
    pub fn new() -> Self {
        NESBus {
            ram: [0; 0x800],
            ppu: PPU::new(),
            ppu_memory: PPUMemory::new(),
            apu: APU::new(),
            dma: DmaController::new(),
            controllers: [Joypad::new(), Joypad::new()],
            cpu_open_bus: 0,
        }
    }

    pub fn ppu(&self) -> &PPU {
        &self.ppu
    }

    pub fn add_expansion_audio_chip(&mut self, chip: Box<dyn crate::apu::ExpansionAudioChip>) {
        self.apu.add_expansion_chip(chip);
    }

    pub fn set_controller_state(&mut self, port: usize, state: ControllerState) {
        if let Some(controller) = self.controllers.get_mut(port) {
            controller.set_state(state);
        }
    }

    pub fn insert_cartridge(&mut self, mut cartridge: Cartridge) {
        let tv_system = cartridge.tv_system();
        self.ppu.set_parameters(tv_system);
        self.apu.set_tv_system(tv_system);
        let chips = cartridge.take_expansion_audio_chips();
        self.ppu_memory.insert_cartridge(cartridge);
        for chip in chips {
            self.apu.add_expansion_chip(chip);
        }
    }

    pub fn load_cartridge_ines(&mut self, rom: &[u8]) -> Result<(), CartridgeError> {
        let cartridge: Cartridge = Cartridge::from_ines(rom)?;
        self.insert_cartridge(cartridge);
        Ok(())
    }

    pub fn reset(&mut self) {
        self.ppu.reset();
        self.apu.reset();
        self.dma = DmaController::new();
        self.cpu_open_bus = 0;
    }

    pub fn tick_ppu(&mut self) {
        let ppu = &mut self.ppu;
        let ppu_memory = &mut self.ppu_memory;
        ppu.clock(ppu_memory);
    }

    pub fn ppu_nmi_line(&self) -> bool {
        self.ppu.nmi_line()
    }

    pub fn ppu_frame(&self) -> u64 {
        self.ppu.frame()
    }

    pub fn tick_apu_cpu_cycle(&mut self) {
        self.apu.tick_cpu_cycle();
        self.ppu_memory.cartridge_tick_cpu_cycle();
    }

    pub fn apu_sample_rate(&self) -> u32 {
        self.apu.sample_rate()
    }

    pub fn set_apu_sample_rate(&mut self, sample_rate: u32) {
        self.apu.set_sample_rate(sample_rate);
    }

    pub fn apu_audio_samples(&self) -> &[f32] {
        self.apu.audio_samples()
    }

    pub fn clear_apu_audio_samples(&mut self) {
        self.apu.clear_audio_samples();
    }

    pub fn set_apu_debug_mute_mask(&mut self, mask: u8) {
        self.apu.set_debug_mute_mask(mask);
    }

    pub fn apu_debug_mute_mask(&self) -> u8 {
        self.apu.debug_mute_mask()
    }

    pub fn apu_irq_line(&self) -> bool {
        self.apu.irq_line()
    }

    pub fn cartridge_irq_line(&self) -> bool {
        self.ppu_memory.cartridge_irq_line()
    }

    #[allow(dead_code)]
    pub fn dma_in_progress(&self) -> bool {
        self.dma.in_progress()
    }

    pub fn advance_dma_cpu_phase(&mut self) {
        self.dma.advance_cpu_phase();
    }

    pub(crate) fn save_state(&self, writer: &mut StateWriter) -> Result<(), SaveStateError> {
        writer.write_bytes(&self.ram);
        self.ppu.save_state(writer);
        self.ppu_memory.save_state(writer)?;
        self.apu.save_state(writer);
        self.dma.save_state(writer);
        for controller in &self.controllers {
            controller.save_state(writer);
        }
        writer.write_u8(self.cpu_open_bus);
        Ok(())
    }

    pub(crate) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.ram)?;
        self.ppu.load_state(reader)?;
        self.ppu_memory.load_state(reader)?;
        self.apu.load_state(reader)?;
        self.dma.load_state(reader)?;
        for controller in &mut self.controllers {
            controller.load_state(reader)?;
        }
        self.cpu_open_bus = reader.read_u8()?;
        Ok(())
    }

    fn latched_cpu_read(&mut self, data: u8) -> u8 {
        self.cpu_open_bus = data;
        data
    }

    fn cpu_read_internal(&mut self, addr: u16, cycle_offset: u8) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.latched_cpu_read(self.ram[(addr & 0x7FF) as usize]),
            0x2000..=0x3FFF => {
                let ppu = &mut self.ppu;
                let ppu_memory = &mut self.ppu_memory;
                let data =
                    ppu.cpu_read_register_timed(ppu_memory, 0x2000 | (addr & 0x0007), cycle_offset);
                self.latched_cpu_read(data)
            }
            0x4015 => {
                let data =
                    self.apu.read_status_at_offset(cycle_offset) | (self.cpu_open_bus & 0x20);
                self.latched_cpu_read(data)
            }
            0x4016 => {
                let data = (self.cpu_open_bus & 0xE0) | self.controllers[0].read();
                self.latched_cpu_read(data)
            }
            0x4017 => {
                let data = (self.cpu_open_bus & 0xE0) | self.controllers[1].read();
                self.latched_cpu_read(data)
            }
            0x4020..=0xFFFF => {
                let data = self
                    .ppu_memory
                    .cartridge_cpu_read(addr)
                    .unwrap_or(self.cpu_open_bus);
                self.latched_cpu_read(data)
            }
            // Handle other address ranges (APU, cartridge, etc.)
            _ => self.cpu_open_bus,
        }
    }

    fn cpu_write_internal(&mut self, addr: u16, data: u8, cycle_offset: u8) {
        self.cpu_open_bus = data;
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x7FF) as usize] = data,
            0x2000..=0x3FFF => {
                let ppu = &mut self.ppu;
                let ppu_memory = &mut self.ppu_memory;
                ppu.cpu_write_register_timed(
                    ppu_memory,
                    0x2000 | (addr & 0x0007),
                    data,
                    cycle_offset,
                );
            }
            0x4014 => self.dma.request_oam_dma(data),
            0x4000..=0x4013 | 0x4015 => self.apu.write_register_at_offset(addr, data, cycle_offset),
            0x4016 => {
                self.controllers[0].write(data);
                self.controllers[1].write(data);
            }
            0x4017 => self.apu.write_register_at_offset(addr, data, cycle_offset),
            0x4020..=0xFFFF => {
                let _ = self.ppu_memory.cartridge_cpu_write(addr, data);
            }
            // Handle other address ranges (APU, cartridge, etc.)
            _ => {}
        }
    }

    fn tick_dma_cpu_cycle(&mut self) -> bool {
        let dmc_request = self.apu.take_dmc_dma_request();
        let (consumed, request) = self.dma.tick_cpu_cycle(dmc_request);
        match request {
            DmaBusRequest::None => {}
            DmaBusRequest::DmcRead { addr } => {
                let data = self.cpu_read_internal(addr, 0);
                self.apu.submit_dmc_dma_sample(data);
                self.dma.apply_dmc_read();
            }
            DmaBusRequest::OamRead { addr } => {
                let data = self.cpu_read_internal(addr, 0);
                self.dma.apply_oam_read(data);
            }
            DmaBusRequest::OamWrite { data } => {
                self.ppu.write_oam_dma(data);
                self.dma.apply_oam_write();
            }
        }
        consumed
    }
}

impl CPUBus for NESBus {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.cpu_read_internal(addr, 0)
    }

    fn cpu_write(&mut self, addr: u16, data: u8) {
        self.cpu_write_internal(addr, data, 0);
    }

    fn cpu_read_timed(&mut self, addr: u16, cycle_offset: u8) -> u8 {
        self.cpu_read_internal(addr, cycle_offset)
    }

    fn cpu_write_timed(&mut self, addr: u16, data: u8, cycle_offset: u8) {
        self.cpu_write_internal(addr, data, cycle_offset);
    }

    fn try_dma(&mut self) -> bool {
        self.tick_dma_cpu_cycle()
    }
}

impl Default for NESBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
