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

#[test]
fn reset_wraps_stack_pointer_and_clears_internal_interrupt_state() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    bus.write_u16(0xFFFC, 0x3456);

    cpu.sp = 0x01;
    cpu.interrupt_delay = true;
    cpu.pre_interrupt_delay = true;
    cpu.nmi_next = true;
    cpu.set_nmi(true);

    cpu.reset(&mut bus);

    assert_eq!(cpu.pc, 0x3456);
    assert_eq!(cpu.sp, 0xFE);
    assert!(cpu.p.i);
    assert!(!cpu.interrupt_delay);
    assert!(!cpu.pre_interrupt_delay);
    assert!(!cpu.nmi_next);
    assert!(cpu.nmi_prev, "reset should synchronize NMI edge tracking");
}
