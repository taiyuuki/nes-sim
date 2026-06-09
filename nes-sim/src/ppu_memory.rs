use crate::cartridge::{Cartridge, Mirroring};
use crate::ppu::PPUBus;
use crate::savestate::{SaveStateError, StateReader, StateWriter};

pub(super) struct PPUMemory {
    chr_ram: [u8; 0x2000],
    vram: [u8; 0x1000],
    palette: [u8; 0x20],
    cartridge: Option<Cartridge>,
}

impl PPUMemory {
    pub(super) fn new() -> Self {
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
            .and_then(|c| c.map_nametable_addr(addr))
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
            .map(|c| c.mirroring())
            .unwrap_or(Mirroring::Horizontal)
    }

    pub(super) fn insert_cartridge(&mut self, cartridge: Cartridge) {
        self.cartridge = Some(cartridge);
    }

    pub(super) fn cartridge_cpu_read(&mut self, addr: u16) -> Option<u8> {
        self.cartridge.as_mut().and_then(|c| c.cpu_read(addr))
    }

    pub(super) fn cartridge_cpu_write(&mut self, addr: u16, data: u8) -> bool {
        self.cartridge
            .as_mut()
            .is_some_and(|c| c.cpu_write(addr, data))
    }

    pub(super) fn cartridge_irq_line(&self) -> bool {
        self.cartridge.as_ref().is_some_and(|c| c.irq_line())
    }

    pub(super) fn cartridge_tick_cpu_cycle(&mut self) {
        if let Some(cartridge) = &mut self.cartridge {
            cartridge.tick_cpu_cycle();
        }
    }

    #[allow(dead_code)]
    pub(super) fn notify_scanline(&mut self, scanline: i16, rendering_on: bool) {
        if let Some(cartridge) = &mut self.cartridge {
            cartridge.notify_scanline(scanline, rendering_on);
        }
    }

    pub(super) fn ppu_read_nametable(&mut self, addr: u16) -> Option<u8> {
        self.cartridge
            .as_mut()
            .and_then(|c| c.ppu_read_nametable(addr))
    }

    pub(super) fn ppu_write_nametable(&mut self, addr: u16, data: u8) -> bool {
        self.cartridge
            .as_mut()
            .is_some_and(|c| c.ppu_write_nametable(addr, data))
    }

    pub(super) fn save_state(&self, writer: &mut StateWriter) -> Result<(), SaveStateError> {
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

    pub(super) fn load_state(
        &mut self,
        reader: &mut StateReader<'_>,
    ) -> Result<(), SaveStateError> {
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
                .and_then(|c| c.ppu_read(addr))
                .unwrap_or_else(|| self.chr_ram[addr as usize]),
            0x2000..=0x3EFF => {
                if let Some(data) = self.ppu_read_nametable(addr) {
                    data
                } else {
                    self.vram[self.nametable_index(addr)]
                }
            }
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
                    .is_some_and(|c| c.ppu_write(addr, data))
                {
                    self.chr_ram[addr as usize] = data;
                }
            }
            0x2000..=0x3EFF => {
                if !self.ppu_write_nametable(addr, data) {
                    self.vram[self.nametable_index(addr)] = data;
                }
            }
            0x3F00..=0x3FFF => self.palette[Self::palette_index(addr)] = data,
            _ => {}
        }
    }

    fn check_a12(&mut self, addr: u16, ppu_cycle: u64) {
        if let Some(cartridge) = &mut self.cartridge {
            cartridge.check_a12(addr, ppu_cycle);
        }
    }

    fn notify_scanline(&mut self, scanline: i16, rendering_on: bool) {
        if let Some(cartridge) = &mut self.cartridge {
            cartridge.notify_scanline(scanline, rendering_on);
        }
    }

    fn set_ppu_sprite_phase(&mut self, sprite_phase: bool) {
        if let Some(cartridge) = &mut self.cartridge {
            cartridge.set_ppu_sprite_phase(sprite_phase);
        }
    }
}
