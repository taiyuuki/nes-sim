pub struct PPU {
    oam: [u8; 256],
    oam_addr: u8,
}

impl PPU {
    pub fn new() -> Self {
        Self {
            oam: [0; 256],
            oam_addr: 0,
        }
    }

    pub fn cpu_read_register(&mut self, addr: u16) -> u8 {
        match addr {
            0x2004 => self.oam[self.oam_addr as usize],
            _ => 0,
        }
    }

    pub fn cpu_write_register(&mut self, addr: u16, data: u8) {
        match addr {
            0x2003 => self.oam_addr = data,
            0x2004 => self.write_oam_data(data),
            _ => {}
        }
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
