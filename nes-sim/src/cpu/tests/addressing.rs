use super::*;

#[test]
fn rel_returns_signed_target_and_advances_pc() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x2000;
    bus.cpu_write(0x2000, 0xFE);

    let addr = cpu.resolve_operand(AddrMode::REL, &mut bus);

    assert_eq!(addr.map(|operand| operand.addr), Some(0x1FFF));
    assert_eq!(cpu.pc, 0x2001);
}

#[test]
fn izx_wraps_zero_page_pointer_before_reading_effective_address() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x3000;
    cpu.x = 0x20;
    bus.cpu_write(0x3000, 0xF0);
    bus.cpu_write(0x0010, 0xCD);
    bus.cpu_write(0x0011, 0xAB);

    let addr = cpu.resolve_operand(AddrMode::IZX, &mut bus);

    assert_eq!(addr.map(|operand| operand.addr), Some(0xABCD));
    assert_eq!(cpu.pc, 0x3001);
}

#[test]
fn ind_emulates_6502_page_wrap_bug() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x4000;
    bus.write_u16(0x4000, 0x12FF);
    bus.cpu_write(0x12FF, 0x78);
    bus.cpu_write(0x1200, 0x56);
    bus.cpu_write(0x1300, 0x99);

    let addr = cpu.resolve_operand(AddrMode::IND, &mut bus);

    assert_eq!(addr.map(|operand| operand.addr), Some(0x5678));
    assert_eq!(cpu.pc, 0x4002);
}

#[test]
fn page_crossed_returns_true_when_addresses_span_pages() {
    assert!(CPU::page_crossed(0x12FF, 0x1300));
    assert!(!CPU::page_crossed(0x1200, 0x12FF));
}
