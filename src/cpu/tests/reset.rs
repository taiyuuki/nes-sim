use super::*;

#[test]
fn sets_pc_stack_pointer_and_interrupt_disable_from_reset_vector() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    bus.write_u16(0xFFFC, 0x1234);

    cpu.reset(&mut bus);

    assert_eq!(cpu.pc, 0x1234);
    assert_eq!(cpu.sp, 0xFA);
    assert!(cpu.p.i, "interrupt disable flag should be set after reset");
}
