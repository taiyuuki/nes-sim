use super::*;

struct TestPpuBus {
    mem: [u8; 0x4000],
}

impl TestPpuBus {
    fn new() -> Self {
        Self { mem: [0; 0x4000] }
    }
}

impl PPUBus for TestPpuBus {
    fn ppu_read(&mut self, addr: u16) -> u8 {
        self.mem[(addr & 0x3FFF) as usize]
    }

    fn ppu_write(&mut self, addr: u16, data: u8) {
        self.mem[(addr & 0x3FFF) as usize] = data;
    }
}

#[test]
fn ppudata_write_uses_configured_increment() {
    let mut ppu = PPU::new();
    let mut bus = TestPpuBus::new();

    ppu.cpu_write_register(&mut bus, 0x2000, CTRL_VRAM_INCREMENT);
    ppu.cpu_write_register(&mut bus, 0x2006, 0x20);
    ppu.cpu_write_register(&mut bus, 0x2006, 0x00);
    ppu.cpu_write_register(&mut bus, 0x2007, 0x12);
    ppu.cpu_write_register(&mut bus, 0x2007, 0x34);

    assert_eq!(bus.mem[0x2000], 0x12);
    assert_eq!(bus.mem[0x2020], 0x34);
}

#[test]
fn reading_ppustatus_clears_vblank_and_resets_write_toggle() {
    let mut ppu = PPU::new();
    let mut bus = TestPpuBus::new();

    ppu.status = STATUS_VBLANK;
    ppu.open_bus = 0x1B;
    ppu.write_latch = true;

    let status = ppu.cpu_read_register(&mut bus, 0x2002);

    assert_eq!(status, 0x9B);
    assert!(!ppu.in_vblank());
    assert!(!ppu.write_latch);
}

#[test]
fn clock_enters_vblank_and_asserts_nmi_line_when_enabled() {
    let mut ppu = PPU::new();
    let mut bus = TestPpuBus::new();

    ppu.cpu_write_register(&mut bus, 0x2000, CTRL_NMI_ENABLE);

    for _ in 0..(341 * 262) {
        if ppu.nmi_line() {
            break;
        }
        ppu.clock(&mut bus);
    }

    assert!(ppu.in_vblank());
    assert!(ppu.nmi_line());
    assert_eq!(ppu.scanline(), 241);
    assert_eq!(ppu.dot(), 1);
}
