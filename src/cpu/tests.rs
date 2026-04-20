use super::*;

pub(super) struct TestBus {
    mem: [u8; 0x10000],
}

impl TestBus {
    pub(super) fn new() -> Self {
        Self { mem: [0; 0x10000] }
    }

    pub(super) fn write_u16(&mut self, addr: u16, value: u16) {
        self.cpu_write(addr, value as u8);
        self.cpu_write(addr.wrapping_add(1), (value >> 8) as u8);
    }
}

impl CPUBus for TestBus {
    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.mem[addr as usize]
    }

    fn cpu_write(&mut self, addr: u16, data: u8) {
        self.mem[addr as usize] = data;
    }
}

mod addressing;
mod dma;
mod execution;
mod reset;
mod unofficial;
