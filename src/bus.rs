pub trait CPUBus {
    fn cpu_read(&mut self, addr: u16) -> u8;
    fn cpu_write(&mut self, addr: u16, data: u8);

    fn cpu_read_u16(&mut self, addr: u16) -> u16 {
        let lo = self.cpu_read(addr) as u16;
        let hi = self.cpu_read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }
}

pub struct NESBus {
    pub ram: [u8; 0x800],
    // Additional components: PPU, APU, etc. can be added here
}

impl NESBus {
    pub fn new() -> Self {
        NESBus { ram: [0; 0x800] }
    }

    pub fn set_ram(&mut self, data: [u8; 0x800]) {
        self.ram = data;
    }

    pub fn run_dma(&mut self, _addr: u16) {
        // DMA transfer from CPU memory to PPU OAM
    }
}

impl CPUBus for NESBus {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x7FF) as usize],
            // Handle other address ranges (PPU, APU, etc.)
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, data: u8) {
        match addr {
            0x0000..=0x1FFF => self.ram[(addr & 0x7FF) as usize] = data,
            // Handle other address ranges (PPU, APU, etc.)
            _ => (),
        }
    }
}
