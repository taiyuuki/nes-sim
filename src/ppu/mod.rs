const STATUS_SPRITE_OVERFLOW: u8 = 0x20;
const STATUS_SPRITE_ZERO_HIT: u8 = 0x40;
const STATUS_VBLANK: u8 = 0x80;
const CTRL_VRAM_INCREMENT: u8 = 0x04;
const CTRL_NMI_ENABLE: u8 = 0x80;

pub trait PPUBus {
    fn ppu_read(&mut self, addr: u16) -> u8;
    fn ppu_write(&mut self, addr: u16, data: u8);
}

pub struct PPU {
    oam: [u8; 256],
    oam_addr: u8,
    ctrl: u8,
    mask: u8,
    status: u8,
    open_bus: u8,
    vram_addr: u16,
    temp_vram_addr: u16,
    fine_x: u8,
    write_latch: bool,
    read_buffer: u8,
    scanline: i16,
    dot: u16,
    odd_frame: bool,
}

impl PPU {
    pub fn new() -> Self {
        Self {
            oam: [0; 256],
            oam_addr: 0,
            ctrl: 0,
            mask: 0,
            status: 0,
            open_bus: 0,
            vram_addr: 0,
            temp_vram_addr: 0,
            fine_x: 0,
            write_latch: false,
            read_buffer: 0,
            scanline: 261,
            dot: 0,
            odd_frame: false,
        }
    }

    pub fn reset(&mut self) {
        self.ctrl = 0;
        self.mask = 0;
        self.status &= !(STATUS_SPRITE_OVERFLOW | STATUS_SPRITE_ZERO_HIT | STATUS_VBLANK);
        self.open_bus = 0;
        self.vram_addr = 0;
        self.temp_vram_addr = 0;
        self.fine_x = 0;
        self.write_latch = false;
        self.read_buffer = 0;
        self.scanline = 261;
        self.dot = 0;
        self.odd_frame = false;
    }

    pub fn cpu_read_register<B: PPUBus>(&mut self, bus: &mut B, addr: u16) -> u8 {
        match addr {
            0x2002 => self.read_status(),
            0x2004 => {
                let data = self.oam[self.oam_addr as usize];
                self.open_bus = data;
                data
            }
            0x2007 => self.read_data(bus),
            _ => self.open_bus,
        }
    }

    pub fn cpu_write_register<B: PPUBus>(&mut self, bus: &mut B, addr: u16, data: u8) {
        self.open_bus = data;

        match addr {
            0x2000 => {
                self.ctrl = data;
                self.temp_vram_addr =
                    (self.temp_vram_addr & !0x0C00) | (((data as u16) & 0x03) << 10);
            }
            0x2001 => self.mask = data,
            0x2003 => self.oam_addr = data,
            0x2004 => self.write_oam_data(data),
            0x2005 => self.write_scroll(data),
            0x2006 => self.write_addr(data),
            0x2007 => self.write_data(bus, data),
            _ => {}
        }
    }

    pub fn clock<B: PPUBus>(&mut self, _bus: &mut B) {
        self.dot += 1;
        if self.dot > 340 {
            self.dot = 0;
            self.scanline += 1;
            if self.scanline > 261 {
                self.scanline = 0;
                self.odd_frame = !self.odd_frame;
            }
        }

        match (self.scanline, self.dot) {
            (241, 1) => self.status |= STATUS_VBLANK,
            (261, 1) => {
                self.status &= !(STATUS_VBLANK | STATUS_SPRITE_ZERO_HIT | STATUS_SPRITE_OVERFLOW);
            }
            _ => {}
        }
    }

    pub fn nmi_line(&self) -> bool {
        (self.ctrl & CTRL_NMI_ENABLE) != 0 && (self.status & STATUS_VBLANK) != 0
    }

    pub fn in_vblank(&self) -> bool {
        (self.status & STATUS_VBLANK) != 0
    }

    pub fn scanline(&self) -> i16 {
        self.scanline
    }

    pub fn dot(&self) -> u16 {
        self.dot
    }

    pub(crate) fn write_oam_dma(&mut self, data: u8) {
        self.write_oam_data(data);
    }

    pub fn oam_byte(&self, index: u8) -> u8 {
        self.oam[index as usize]
    }

    pub fn oam_addr(&self) -> u8 {
        self.oam_addr
    }

    fn read_status(&mut self) -> u8 {
        let status = (self.status & 0xE0) | (self.open_bus & 0x1F);
        self.status &= !STATUS_VBLANK;
        self.write_latch = false;
        self.open_bus = status;
        status
    }

    fn read_data<B: PPUBus>(&mut self, bus: &mut B) -> u8 {
        let addr = self.vram_addr & 0x3FFF;
        let data = if addr >= 0x3F00 {
            self.read_buffer = bus.ppu_read(addr.wrapping_sub(0x1000));
            bus.ppu_read(addr)
        } else {
            let buffered = self.read_buffer;
            self.read_buffer = bus.ppu_read(addr);
            buffered
        };

        self.increment_vram_addr();
        self.open_bus = data;
        data
    }

    fn write_data<B: PPUBus>(&mut self, bus: &mut B, data: u8) {
        let addr = self.vram_addr & 0x3FFF;
        bus.ppu_write(addr, data);
        self.increment_vram_addr();
    }

    fn write_scroll(&mut self, data: u8) {
        if !self.write_latch {
            self.fine_x = data & 0x07;
            self.temp_vram_addr = (self.temp_vram_addr & !0x001F) | ((data as u16) >> 3);
            self.write_latch = true;
        } else {
            self.temp_vram_addr = (self.temp_vram_addr & !0x73E0)
                | ((((data as u16) >> 3) & 0x1F) << 5)
                | (((data as u16) & 0x07) << 12);
            self.write_latch = false;
        }
    }

    fn write_addr(&mut self, data: u8) {
        if !self.write_latch {
            self.temp_vram_addr = (self.temp_vram_addr & 0x00FF) | (((data as u16) & 0x3F) << 8);
            self.write_latch = true;
        } else {
            self.temp_vram_addr = (self.temp_vram_addr & 0x7F00) | data as u16;
            self.vram_addr = self.temp_vram_addr;
            self.write_latch = false;
        }
    }

    fn increment_vram_addr(&mut self) {
        let increment = if (self.ctrl & CTRL_VRAM_INCREMENT) != 0 {
            32
        } else {
            1
        };
        self.vram_addr = self.vram_addr.wrapping_add(increment);
    }

    fn write_oam_data(&mut self, data: u8) {
        self.oam[self.oam_addr as usize] = data;
        self.oam_addr = self.oam_addr.wrapping_add(1);
    }
}

impl Default for PPU {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
