use super::*;

#[test]
fn cpu_step_executes_ahx_absolute_y_and_consumes_two_operand_bytes() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0xFF;
    cpu.x = 0xFF;
    cpu.y = 0x01;
    bus.cpu_write(0x0200, 0x9F);
    bus.write_u16(0x0201, 0x1234);
    cpu.clock(&mut bus);

    assert_eq!(cpu.pc, 0x0203);
    assert_eq!(cpu.cycles, 5);
}

#[test]
fn cpu_step_executes_las_absolute_y_and_consumes_two_operand_bytes() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.sp = 0xF0;
    cpu.y = 0x02;
    bus.cpu_write(0x0200, 0xBB);
    bus.write_u16(0x0201, 0x2000);
    bus.cpu_write(0x2002, 0x3C);

    cpu.clock(&mut bus);

    assert_eq!(cpu.pc, 0x0203);
    assert_eq!(cpu.cycles, 4);
}

#[test]
fn cpu_step_executes_anc_immediate_and_consumes_operand_byte() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0xF0;
    bus.cpu_write(0x0200, 0x0B);
    bus.cpu_write(0x0201, 0x80);

    cpu.clock(&mut bus);

    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.cycles, 2);
    assert_eq!(cpu.a, 0x80);
    assert!(cpu.p.c);
    assert!(cpu.p.n);
}

#[test]
fn cpu_step_executes_lax_immediate_and_consumes_operand_byte() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    bus.cpu_write(0x0200, 0xAB);
    bus.cpu_write(0x0201, 0x5A);

    cpu.clock(&mut bus);

    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.cycles, 2);
    assert_eq!(cpu.a, 0x5A);
    assert_eq!(cpu.x, 0x5A);
}

#[test]
fn cpu_step_executes_shy_absolute_x_without_debug_overflow() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.x = 0x01;
    cpu.y = 0xFF;
    bus.cpu_write(0x0200, 0x9C);
    bus.write_u16(0x0201, 0x00FF);

    cpu.clock(&mut bus);

    assert_eq!(cpu.pc, 0x0203);
    assert_eq!(cpu.cycles, 5);
    assert_eq!(bus.cpu_read(0x0100), 0x02);
}

#[test]
fn cpu_step_executes_tas_absolute_y_without_debug_overflow() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0xFF;
    cpu.x = 0xFF;
    cpu.y = 0x60;
    cpu.sp = 0xFD;
    bus.cpu_write(0x0200, 0x9B);
    bus.write_u16(0x0201, 0xFEF0);

    cpu.clock(&mut bus);

    assert_eq!(cpu.pc, 0x0203);
    assert_eq!(cpu.cycles, 5);
}

#[test]
fn cpu_step_executes_ahx_absolute_y_without_debug_overflow() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0xFF;
    cpu.x = 0xFF;
    cpu.y = 0x60;
    bus.cpu_write(0x0200, 0x9F);
    bus.write_u16(0x0201, 0xFEF0);

    cpu.clock(&mut bus);

    assert_eq!(cpu.pc, 0x0203);
    assert_eq!(cpu.cycles, 5);
}

#[test]
fn cpu_step_executes_xaa_immediate_to_zero_without_touching_x() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x5A;
    cpu.x = 0x3B;
    cpu.p.i = true;
    cpu.p.c = true;
    bus.cpu_write(0x0200, 0x8B);
    bus.cpu_write(0x0201, 0x00);

    cpu.clock(&mut bus);

    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.cycles, 2);
    assert_eq!(cpu.a, 0x00);
    assert_eq!(cpu.x, 0x3B);
    assert!(cpu.p.z);
    assert!(!cpu.p.n);
    assert!(cpu.p.i);
    assert!(cpu.p.c);
}

#[test]
fn cpu_step_executes_xaa_immediate_as_and_of_a_x_and_operand() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0xFF;
    cpu.x = 0x9F;
    cpu.p.i = true;
    cpu.p.v = true;
    bus.cpu_write(0x0200, 0x8B);
    bus.cpu_write(0x0201, 0xCF);

    cpu.clock(&mut bus);

    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.cycles, 2);
    assert_eq!(cpu.a, 0x8F);
    assert_eq!(cpu.x, 0x9F);
    assert!(!cpu.p.z);
    assert!(cpu.p.n);
    assert!(cpu.p.i);
    assert!(cpu.p.v);
}
