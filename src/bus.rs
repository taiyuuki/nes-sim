use crate::apu::APU;
use crate::cartridge::{Cartridge, CartridgeError, Mirroring, TVSystem};
use crate::dma::DmaController;
use crate::input::{ControllerState, Joypad};
use crate::ppu::{PPU, PPUBus};
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

struct PPUMemory {
    chr_ram: [u8; 0x2000],
    vram: [u8; 0x1000],
    palette: [u8; 0x20],
    cartridge: Option<Cartridge>,
}

impl PPUMemory {
    fn new() -> Self {
        Self {
            chr_ram: [0; 0x2000],
            vram: [0; 0x1000],
            palette: [0; 0x20],
            cartridge: None,
        }
    }

    fn normalize_addr(addr: u16) -> u16 {
        addr & 0x3FFF
    }

    fn palette_index(addr: u16) -> usize {
        let mut index = (addr - 0x3F00) & 0x001F;
        if matches!(index, 0x10 | 0x14 | 0x18 | 0x1C) {
            index -= 0x10;
        }
        index as usize
    }

    fn nametable_index(&self, addr: u16) -> usize {
        if let Some(index) = self
            .cartridge
            .as_ref()
            .and_then(|cartridge| cartridge.map_nametable_addr(addr))
        {
            return index;
        }

        let offset = (addr - 0x2000) & 0x0FFF;
        let table = offset / 0x0400;
        let inner = (offset & 0x03FF) as usize;

        match self.mirroring() {
            Mirroring::Horizontal => match table {
                0 | 1 => inner,
                2 | 3 => 0x0400 + inner,
                _ => unreachable!(),
            },
            Mirroring::Vertical => match table {
                0 | 2 => inner,
                1 | 3 => 0x0400 + inner,
                _ => unreachable!(),
            },
            Mirroring::SPAGE0 => inner,
            Mirroring::SPAGE1 => 0x0400 + inner,
            Mirroring::FourScreen => offset as usize,
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.cartridge
            .as_ref()
            .map(Cartridge::mirroring)
            .unwrap_or(Mirroring::Horizontal)
    }

    fn insert_cartridge(&mut self, cartridge: Cartridge) {
        self.cartridge = Some(cartridge);
    }

    fn cartridge_cpu_read(&mut self, addr: u16) -> Option<u8> {
        self.cartridge
            .as_mut()
            .and_then(|cartridge| cartridge.cpu_read(addr))
    }

    fn cartridge_cpu_write(&mut self, addr: u16, data: u8) -> bool {
        self.cartridge
            .as_mut()
            .is_some_and(|cartridge| cartridge.cpu_write(addr, data))
    }

    fn cartridge_irq_line(&self) -> bool {
        self.cartridge
            .as_ref()
            .is_some_and(|cartridge| cartridge.irq_line())
    }

    fn save_state(&self, writer: &mut StateWriter) -> Result<(), SaveStateError> {
        writer.write_bytes(&self.chr_ram);
        writer.write_bytes(&self.vram);
        writer.write_bytes(&self.palette);
        match &self.cartridge {
            Some(cartridge) => {
                writer.write_bool(true);
                cartridge.save_state(writer);
                Ok(())
            }
            None => Err(SaveStateError::NoCartridge),
        }
    }

    fn load_state(&mut self, reader: &mut StateReader<'_>) -> Result<(), SaveStateError> {
        reader.read_bytes_into(&mut self.chr_ram)?;
        reader.read_bytes_into(&mut self.vram)?;
        reader.read_bytes_into(&mut self.palette)?;
        let has_cartridge = reader.read_bool()?;
        match (&mut self.cartridge, has_cartridge) {
            (Some(cartridge), true) => cartridge.load_state(reader),
            (None, _) => Err(SaveStateError::NoCartridge),
            _ => Err(SaveStateError::InvalidData(
                "save state expected a loaded cartridge",
            )),
        }
    }
}

impl Default for PPUMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl PPUBus for PPUMemory {
    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = Self::normalize_addr(addr);
        match addr {
            0x0000..=0x1FFF => self
                .cartridge
                .as_mut()
                .and_then(|cartridge| cartridge.ppu_read(addr))
                .unwrap_or_else(|| self.chr_ram[addr as usize]),
            0x2000..=0x3EFF => self.vram[self.nametable_index(addr)],
            0x3F00..=0x3FFF => self.palette[Self::palette_index(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, data: u8) {
        let addr = Self::normalize_addr(addr);
        match addr {
            0x0000..=0x1FFF => {
                if !self
                    .cartridge
                    .as_mut()
                    .is_some_and(|cartridge| cartridge.ppu_write(addr, data))
                {
                    self.chr_ram[addr as usize] = data;
                }
            }
            0x2000..=0x3EFF => self.vram[self.nametable_index(addr)] = data,
            0x3F00..=0x3FFF => self.palette[Self::palette_index(addr)] = data,
            _ => {}
        }
    }

    fn check_a12(&mut self, addr: u16, ppu_cycle: u64) {
        if let Some(cartridge) = &mut self.cartridge {
            cartridge.check_a12(addr, ppu_cycle);
        }
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

    pub fn set_ram(&mut self, data: [u8; 0x800]) {
        self.ram = data;
    }

    pub fn ppu(&self) -> &PPU {
        &self.ppu
    }

    pub fn set_controller_state(&mut self, port: usize, state: ControllerState) {
        if let Some(controller) = self.controllers.get_mut(port) {
            controller.set_state(state);
        }
    }

    pub fn insert_cartridge(&mut self, cartridge: Cartridge) {
        self.ppu.set_parameters(cartridge.tv_system());
        self.ppu_memory.insert_cartridge(cartridge);
    }

    pub fn load_cartridge_ines(&mut self, rom: &[u8]) -> Result<(), CartridgeError> {
        self.load_cartridge_ines_with_tv_system_override(rom, None)
    }

    pub fn load_cartridge_ines_with_tv_system_override(
        &mut self,
        rom: &[u8],
        tv_system_override: Option<TVSystem>,
    ) -> Result<(), CartridgeError> {
        let cartridge: Cartridge =
            Cartridge::from_ines_with_tv_system_override(rom, tv_system_override)?;
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
    }

    pub fn apu_sample_rate(&self) -> u32 {
        self.apu.sample_rate()
    }

    pub fn apu_audio_samples(&self) -> &[f32] {
        self.apu.audio_samples()
    }

    pub fn clear_apu_audio_samples(&mut self) {
        self.apu.clear_audio_samples();
    }

    pub fn apu_irq_line(&self) -> bool {
        self.apu.irq_line()
    }

    pub fn cartridge_irq_line(&self) -> bool {
        self.ppu_memory.cartridge_irq_line()
    }

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

    pub(crate) fn dma_read(&mut self, addr: u16) -> u8 {
        self.cpu_read_internal(addr, 0)
    }

    pub(crate) fn dma_write_oam(&mut self, data: u8) {
        self.ppu.write_oam_dma(data);
    }

    pub(crate) fn take_dmc_dma_request(&mut self) -> Option<crate::apu::DmcDmaRequest> {
        self.apu.take_dmc_dma_request()
    }

    pub(crate) fn submit_dmc_dma_sample(&mut self, data: u8) {
        self.apu.submit_dmc_dma_sample(data);
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
            0x4015 => self.apu.read_status_at_offset(cycle_offset) | (self.cpu_open_bus & 0x20),
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
        let mut dma = std::mem::take(&mut self.dma);
        let consumed = dma.tick_cpu_cycle(self);
        self.dma = dma;
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
mod tests {
    use super::*;
    use crate::input::{ControllerButton, ControllerState};

    fn write_mmc1_register(bus: &mut NESBus, addr: u16, value: u8) {
        for bit in 0..5 {
            bus.cpu_write(addr, (value >> bit) & 0x01);
        }
    }

    fn make_ines(prg_banks: u8, chr_banks: u8, flags6: u8) -> Vec<u8> {
        let mut rom = vec![0; 16];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = prg_banks;
        rom[5] = chr_banks;
        rom[6] = flags6;

        let prg_len = prg_banks as usize * 0x4000;
        let chr_len = chr_banks as usize * 0x2000;
        rom.extend((0..prg_len).map(|index| (index & 0xFF) as u8));
        rom.extend((0..chr_len).map(|index| (0x80 | (index & 0x7F)) as u8));
        rom
    }

    fn make_ines_mapper(prg_banks: u8, chr_banks: u8, mapper_id: u16, flags6_low: u8) -> Vec<u8> {
        let mut rom = make_ines(
            prg_banks,
            chr_banks,
            ((mapper_id as u8) << 4) | (flags6_low & 0x0F),
        );
        rom[7] = (mapper_id & 0xF0) as u8;
        rom
    }

    #[test]
    fn cartridge_prg_rom_is_visible_on_cpu_bus() {
        let mut bus = NESBus::new();
        let rom = make_ines(1, 1, 0x00);

        bus.load_cartridge_ines(&rom).expect("NROM should load");

        assert_eq!(bus.cpu_read(0x8000), 0x00);
        assert_eq!(bus.cpu_read(0x8001), 0x01);
        assert_eq!(bus.cpu_read(0xC000), 0x00);
        assert_eq!(bus.cpu_read(0xFFFF), 0xFF);
    }

    #[test]
    fn cartridge_chr_rom_is_visible_through_ppu_registers() {
        let mut bus = NESBus::new();
        let rom = make_ines(1, 1, 0x00);

        bus.load_cartridge_ines(&rom).expect("NROM should load");
        bus.cpu_write(0x2006, 0x00);
        bus.cpu_write(0x2006, 0x00);

        assert_eq!(
            bus.cpu_read(0x2007),
            0x00,
            "first read should return the old buffer"
        );
        assert_eq!(
            bus.cpu_read(0x2007),
            0x80,
            "second read should return CHR byte 0"
        );
        assert_eq!(
            bus.cpu_read(0x2007),
            0x81,
            "buffer should advance through CHR"
        );
    }

    #[test]
    fn nametable_access_uses_cartridge_mirroring() {
        let mut bus = NESBus::new();
        let rom = make_ines(1, 0, 0x01);

        bus.load_cartridge_ines(&rom)
            .expect("vertical mirroring NROM should load");

        bus.cpu_write(0x2006, 0x20);
        bus.cpu_write(0x2006, 0x00);
        bus.cpu_write(0x2007, 0x3C);

        bus.cpu_write(0x2006, 0x28);
        bus.cpu_write(0x2006, 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0x3C);
    }

    #[test]
    fn nametable_access_tracks_mmc1_runtime_mirroring_changes() {
        let mut bus = NESBus::new();
        let rom = make_ines(2, 0, 0x10);

        bus.load_cartridge_ines(&rom).expect("MMC1 should load");

        bus.cpu_write(0x2006, 0x20);
        bus.cpu_write(0x2006, 0x00);
        bus.cpu_write(0x2007, 0x5A);

        bus.cpu_write(0x2006, 0x24);
        bus.cpu_write(0x2006, 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0x5A);

        write_mmc1_register(&mut bus, 0x8000, 0x02);

        bus.cpu_write(0x2006, 0x20);
        bus.cpu_write(0x2006, 0x00);
        bus.cpu_write(0x2007, 0xA5);

        bus.cpu_write(0x2006, 0x28);
        bus.cpu_write(0x2006, 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0xA5);
    }

    #[test]
    fn nametable_access_tracks_mapper118_chr_bank_mirroring_in_2k_mode() {
        let mut bus = NESBus::new();
        let rom = make_ines_mapper(2, 1, 118, 0x00);

        bus.load_cartridge_ines(&rom)
            .expect("Mapper 118 should load");

        bus.cpu_write(0x8000, 0x00);
        bus.cpu_write(0x8001, 0x00);
        bus.cpu_write(0x8000, 0x01);
        bus.cpu_write(0x8001, 0x80);

        bus.cpu_write(0x2006, 0x20);
        bus.cpu_write(0x2006, 0x00);
        bus.cpu_write(0x2007, 0x11);

        bus.cpu_write(0x2006, 0x28);
        bus.cpu_write(0x2006, 0x00);
        bus.cpu_write(0x2007, 0x22);

        bus.cpu_write(0x2006, 0x24);
        bus.cpu_write(0x2006, 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0x11);

        bus.cpu_write(0x2006, 0x2C);
        bus.cpu_write(0x2006, 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0x22);

        bus.cpu_write(0xA000, 0x00);
        bus.cpu_write(0x2006, 0x24);
        bus.cpu_write(0x2006, 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0x11);
    }

    #[test]
    fn nametable_access_tracks_mapper118_chr_bank_mirroring_in_1k_mode() {
        let mut bus = NESBus::new();
        let rom = make_ines_mapper(2, 1, 118, 0x00);

        bus.load_cartridge_ines(&rom)
            .expect("Mapper 118 should load");

        bus.cpu_write(0x8000, 0x82);
        bus.cpu_write(0x8001, 0x80);
        bus.cpu_write(0x8000, 0x83);
        bus.cpu_write(0x8001, 0x00);
        bus.cpu_write(0x8000, 0x84);
        bus.cpu_write(0x8001, 0x80);
        bus.cpu_write(0x8000, 0x85);
        bus.cpu_write(0x8001, 0x00);

        bus.cpu_write(0x2006, 0x20);
        bus.cpu_write(0x2006, 0x00);
        bus.cpu_write(0x2007, 0xA1);

        bus.cpu_write(0x2006, 0x24);
        bus.cpu_write(0x2006, 0x00);
        bus.cpu_write(0x2007, 0xB2);

        bus.cpu_write(0x2006, 0x28);
        bus.cpu_write(0x2006, 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0xA1);

        bus.cpu_write(0x2006, 0x2C);
        bus.cpu_write(0x2006, 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0x00);
        assert_eq!(bus.cpu_read(0x2007), 0xB2);
    }

    #[test]
    fn controller_reads_shift_latched_buttons_in_standard_order() {
        let mut bus = NESBus::new();
        let mut state = ControllerState::new();
        state.set_pressed(ControllerButton::A, true);
        state.set_pressed(ControllerButton::Select, true);
        state.set_pressed(ControllerButton::Left, true);
        bus.set_controller_state(0, state);

        bus.cpu_write(0x4016, 0x01);
        bus.cpu_write(0x4016, 0x00);

        let reads: Vec<u8> = (0..8).map(|_| bus.cpu_read(0x4016)).collect();

        assert_eq!(reads, vec![1, 0, 1, 0, 0, 0, 1, 0]);
        assert_eq!(bus.cpu_read(0x4016), 1);
        assert_eq!(bus.cpu_read(0x4016), 1);
    }

    #[test]
    fn controller_strobe_high_keeps_reporting_live_a_button_without_advancing() {
        let mut bus = NESBus::new();
        bus.set_controller_state(0, ControllerState::from_bits(0x01));
        bus.cpu_write(0x4016, 0x01);

        assert_eq!(bus.cpu_read(0x4016), 1);
        assert_eq!(bus.cpu_read(0x4016), 1);

        bus.set_controller_state(0, ControllerState::from_bits(0x00));

        assert_eq!(bus.cpu_read(0x4016), 0);
    }

    #[test]
    fn second_controller_reads_from_4017() {
        let mut bus = NESBus::new();
        let mut state = ControllerState::new();
        state.set_pressed(ControllerButton::B, true);
        bus.set_controller_state(1, state);

        bus.cpu_write(0x4016, 0x01);
        bus.cpu_write(0x4016, 0x00);

        assert_eq!(bus.cpu_read(0x4017), 0);
        assert_eq!(bus.cpu_read(0x4017), 1);
    }

    #[test]
    fn unmapped_cpu_reads_return_the_open_bus_value() {
        let mut bus = NESBus::new();

        bus.cpu_write(0x0000, 0x5A);

        assert_eq!(bus.cpu_read(0x4018), 0x5A);
    }

    #[test]
    fn controller_reads_preserve_open_bus_in_upper_bits() {
        let mut bus = NESBus::new();
        bus.set_controller_state(0, ControllerState::from_bits(0x01));
        bus.cpu_write(0x4016, 0x01);
        bus.cpu_write(0x4016, 0x00);
        bus.cpu_write(0x0000, 0xE0);

        assert_eq!(bus.cpu_read(0x4016), 0xE1);
    }

    #[test]
    fn apu_frame_irq_flag_is_visible_in_4015_and_clears_on_read() {
        let mut bus = NESBus::new();
        bus.cpu_write(0x4017, 0x00);

        for _ in 0..30_000 {
            bus.tick_apu_cpu_cycle();
        }

        assert_eq!(bus.cpu_read(0x4015) & 0x40, 0x40);
        for _ in 0..8 {
            bus.tick_apu_cpu_cycle();
        }
        assert_eq!(bus.cpu_read(0x4015) & 0x40, 0x00);
    }

    #[test]
    fn apu_frame_irq_inhibit_write_clears_pending_flag() {
        let mut bus = NESBus::new();
        bus.cpu_write(0x4017, 0x00);

        for _ in 0..29_832 {
            bus.tick_apu_cpu_cycle();
        }

        bus.cpu_write(0x4017, 0x40);

        assert_eq!(bus.cpu_read(0x4015) & 0x40, 0x00);
    }
}
