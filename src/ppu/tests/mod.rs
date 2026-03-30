use super::*;

struct TestPPUBus {
    mem: [u8; 0x4000],
}

impl TestPPUBus {
    fn new() -> Self {
        Self { mem: [0; 0x4000] }
    }
}

impl PPUBus for TestPPUBus {
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
    let mut bus = TestPPUBus::new();

    ppu.cpu_write_register(&mut bus, 0x2000, CTRL_VRAM_INCREMENT);
    ppu.cpu_write_register(&mut bus, 0x2006, 0x20);
    ppu.cpu_write_register(&mut bus, 0x2006, 0x00);
    ppu.cpu_write_register(&mut bus, 0x2007, 0x12);
    ppu.cpu_write_register(&mut bus, 0x2007, 0x34);

    assert_eq!(bus.mem[0x2000], 0x12);
    assert_eq!(bus.mem[0x2020], 0x34);
}

#[test]
fn writing_ppuctrl_with_nmi_enabled_should_assert_nmi_during_vblank() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.status = STATUS_VBLANK;

    ppu.cpu_write_register(&mut bus, 0x2000, CTRL_NMI_ENABLE);

    assert!(ppu.nmi_line(), "NMI line should be asserted during VBlank");
}

#[test]
fn writing_ppuctrl_with_nmi_disabled_should_not_assert_nmi_during_vblank() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.status = STATUS_VBLANK;

    ppu.cpu_write_register(&mut bus, 0x2000, 0x00);

    assert!(
        !ppu.nmi_line(),
        "NMI line should remain low when PPUCTRL bit 7 is clear"
    );
}

#[test]
fn reading_ppustatus_should_clear_vblank_flag() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.status = STATUS_VBLANK;
    ppu.open_bus = 0x1B;

    let status = ppu.cpu_read_register(&mut bus, 0x2002);

    assert_eq!(status, 0x9B);
    assert!(!ppu.in_vblank(), "reading PPUSTATUS should clear VBlank");
}

#[test]
fn reading_ppustatus_should_reset_write_toggle() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

    ppu.write_latch = true;

    let _ = ppu.cpu_read_register(&mut bus, 0x2002);

    assert!(
        !ppu.write_latch,
        "reading PPUSTATUS should clear the write toggle"
    );
}

#[test]
fn clock_enters_vblank_and_asserts_nmi_line_when_enabled() {
    let mut ppu = PPU::new();
    let mut bus = TestPPUBus::new();

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
