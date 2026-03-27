use super::*;

#[test]
fn cpu_step_executes_ora_immediate_and_updates_flags_and_timing() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x80;
    bus.cpu_write(0x0200, 0x09);
    bus.cpu_write(0x0201, 0x01);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.a, 0x81);
    assert_eq!(cpu.pc, 0x0202);
    assert!(!cpu.p.z, "accumulator should not be zero");
    assert!(cpu.p.n, "bit 7 should set the negative flag");
    assert_eq!(cpu.cycles, 2);
    assert_eq!(cpu.clocks, 1);
}

#[test]
fn cpu_step_executes_lda_immediate_and_sets_zero_flag() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    bus.cpu_write(0x0200, 0xA9);
    bus.cpu_write(0x0201, 0x00);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.a, 0x00);
    assert!(cpu.p.z, "loading zero should set the zero flag");
    assert!(!cpu.p.n, "loading zero should clear the negative flag");
    assert_eq!(cpu.pc, 0x0202);
}

#[test]
fn cpu_step_executes_sta_zero_page_and_writes_accumulator_to_bus() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x5A;
    bus.cpu_write(0x0200, 0x85);
    bus.cpu_write(0x0201, 0x10);

    cpu.cpu_step(&mut bus);

    assert_eq!(bus.cpu_read(0x0010), 0x5A);
    assert_eq!(cpu.pc, 0x0202);
}

#[test]
fn cpu_step_executes_tax_and_updates_zero_and_negative_flags() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x80;
    bus.cpu_write(0x0200, 0xAA);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.x, 0x80);
    assert!(!cpu.p.z, "copied value is not zero");
    assert!(cpu.p.n, "copied value has bit 7 set");
    assert_eq!(cpu.pc, 0x0201);
}

#[test]
fn cpu_step_executes_pha_and_pushes_accumulator_to_stack() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x5A;
    cpu.sp = 0xFD;
    bus.cpu_write(0x0200, 0x48);

    cpu.cpu_step(&mut bus);

    assert_eq!(bus.cpu_read(0x01FD), 0x5A);
    assert_eq!(cpu.sp, 0xFC);
    assert_eq!(cpu.pc, 0x0201);
    assert_eq!(cpu.cycles, 3);
    assert_eq!(cpu.clocks, 1);
}

#[test]
fn cpu_step_executes_pla_and_restores_accumulator_and_zero_flag() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.sp = 0xFC;
    cpu.a = 0x7F;
    cpu.p.z = false;
    cpu.p.n = true;
    bus.cpu_write(0x0200, 0x68);
    bus.cpu_write(0x01FD, 0x00);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.a, 0x00);
    assert_eq!(cpu.sp, 0xFD);
    assert!(cpu.p.z, "pulling zero should set the zero flag");
    assert!(!cpu.p.n, "pulling zero should clear the negative flag");
    assert_eq!(cpu.pc, 0x0201);
    assert_eq!(cpu.cycles, 4);
    assert_eq!(cpu.clocks, 1);
}

#[test]
fn cpu_step_executes_php_and_pushes_status_with_break_and_unused_bits_set() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.sp = 0xFD;
    cpu.p.c = true;
    cpu.p.z = true;
    cpu.p.i = false;
    cpu.p.v = true;
    cpu.p.n = true;
    bus.cpu_write(0x0200, 0x08);

    cpu.cpu_step(&mut bus);

    assert_eq!(bus.cpu_read(0x01FD), 0xF3);
    assert_eq!(cpu.sp, 0xFC);
    assert_eq!(cpu.pc, 0x0201);
    assert_eq!(cpu.cycles, 3);
    assert_eq!(cpu.clocks, 1);
}

#[test]
fn cpu_step_executes_plp_and_restores_status_flags_from_stack() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.sp = 0xFC;
    cpu.p.c = false;
    cpu.p.z = false;
    cpu.p.i = false;
    cpu.p.v = false;
    cpu.p.n = false;
    bus.cpu_write(0x0200, 0x28);
    bus.cpu_write(0x01FD, 0xD7);

    cpu.cpu_step(&mut bus);

    assert!(cpu.p.c);
    assert!(cpu.p.z);
    assert!(cpu.p.i);
    assert!(cpu.p.v);
    assert!(cpu.p.n);
    assert_eq!(cpu.sp, 0xFD);
    assert_eq!(cpu.pc, 0x0201);
    assert_eq!(cpu.cycles, 4);
    assert_eq!(cpu.clocks, 1);
}

#[test]
fn cpu_step_executes_plp_and_ignores_break_and_unused_bits() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.sp = 0xFC;
    cpu.p.c = true;
    cpu.p.z = true;
    cpu.p.i = true;
    cpu.p.v = true;
    cpu.p.n = true;
    bus.cpu_write(0x0200, 0x28);
    bus.cpu_write(0x01FD, 0x30);

    cpu.cpu_step(&mut bus);

    assert!(!cpu.p.c);
    assert!(!cpu.p.z);
    assert!(!cpu.p.i);
    assert!(!cpu.p.v);
    assert!(!cpu.p.n);
    assert_eq!(cpu.sp, 0xFD);
    assert_eq!(cpu.pc, 0x0201);
}

#[test]
fn cpu_step_executes_txs_and_copies_x_to_stack_pointer_without_touching_flags() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.x = 0x80;
    cpu.sp = 0xFD;
    cpu.p.z = true;
    cpu.p.n = false;
    bus.cpu_write(0x0200, 0x9A);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.sp, 0x80);
    assert_eq!(cpu.x, 0x80);
    assert!(cpu.p.z, "TXS should not modify zero flag");
    assert!(!cpu.p.n, "TXS should not modify negative flag");
    assert_eq!(cpu.pc, 0x0201);
    assert_eq!(cpu.cycles, 2);
    assert_eq!(cpu.clocks, 1);
}

#[test]
fn cpu_step_executes_tsx_and_updates_zero_and_negative_flags() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.sp = 0x00;
    cpu.x = 0xFF;
    cpu.p.z = false;
    cpu.p.n = true;
    bus.cpu_write(0x0200, 0xBA);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.x, 0x00);
    assert_eq!(cpu.sp, 0x00);
    assert!(cpu.p.z, "copying zero stack pointer should set zero flag");
    assert!(
        !cpu.p.n,
        "copying zero stack pointer should clear negative flag"
    );
    assert_eq!(cpu.pc, 0x0201);
    assert_eq!(cpu.cycles, 2);
    assert_eq!(cpu.clocks, 1);
}

#[test]
fn cpu_step_executes_cmp_immediate_and_sets_carry_zero_and_negative_flags() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x40;
    bus.cpu_write(0x0200, 0xC9);
    bus.cpu_write(0x0201, 0x40);

    cpu.cpu_step(&mut bus);

    assert!(cpu.p.c, "equal compare should set carry");
    assert!(cpu.p.z, "equal compare should set zero");
    assert!(!cpu.p.n, "equal compare should clear negative");
    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.cycles, 2);
}

#[test]
fn cpu_step_executes_cpx_immediate_and_sets_negative_when_register_is_smaller() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.x = 0x10;
    bus.cpu_write(0x0200, 0xE0);
    bus.cpu_write(0x0201, 0x20);

    cpu.cpu_step(&mut bus);

    assert!(!cpu.p.c, "smaller compare should clear carry");
    assert!(!cpu.p.z, "different compare should clear zero");
    assert!(cpu.p.n, "0x10 - 0x20 should set negative");
    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.cycles, 2);
}

#[test]
fn cpu_step_executes_cpy_absolute_and_sets_carry_without_zero() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.y = 0x50;
    bus.cpu_write(0x0200, 0xCC);
    bus.write_u16(0x0201, 0x3456);
    bus.cpu_write(0x3456, 0x10);

    cpu.cpu_step(&mut bus);

    assert!(cpu.p.c, "larger compare should set carry");
    assert!(!cpu.p.z, "different compare should clear zero");
    assert!(!cpu.p.n, "0x50 - 0x10 should clear negative");
    assert_eq!(cpu.pc, 0x0203);
    assert_eq!(cpu.cycles, 4);
}

#[test]
fn cpu_step_executes_cmp_absolute_x_and_adds_cycle_when_page_is_crossed() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x77;
    cpu.x = 0x01;
    bus.cpu_write(0x0200, 0xDD);
    bus.write_u16(0x0201, 0x12FF);
    bus.cpu_write(0x1300, 0x77);

    cpu.cpu_step(&mut bus);

    assert!(cpu.p.c);
    assert!(cpu.p.z);
    assert_eq!(cpu.pc, 0x0203);
    assert_eq!(cpu.cycles, 5);
}

#[test]
fn cpu_step_executes_cmp_indirect_y_and_adds_cycle_when_page_is_crossed() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x20;
    cpu.y = 0x01;
    bus.cpu_write(0x0200, 0xD1);
    bus.cpu_write(0x0201, 0x80);
    bus.cpu_write(0x0080, 0xFF);
    bus.cpu_write(0x0081, 0x12);
    bus.cpu_write(0x1300, 0x10);

    cpu.cpu_step(&mut bus);

    assert!(cpu.p.c);
    assert!(!cpu.p.z);
    assert!(!cpu.p.n);
    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.cycles, 6);
}

#[test]
fn cpu_step_executes_adc_immediate_and_sets_carry_and_zero() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0xFF;
    cpu.p.c = false;
    bus.cpu_write(0x0200, 0x69);
    bus.cpu_write(0x0201, 0x01);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.a, 0x00);
    assert!(cpu.p.c, "0xFF + 0x01 should set carry");
    assert!(cpu.p.z, "wrapped result should set zero");
    assert!(!cpu.p.n, "wrapped result should clear negative");
    assert!(!cpu.p.v, "0xFF + 0x01 should not set overflow");
    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.cycles, 2);
}

#[test]
fn cpu_step_executes_asl_accumulator_and_updates_flags() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x80;
    bus.cpu_write(0x0200, 0x0A);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.a, 0x00);
    assert!(cpu.p.c, "bit 7 should move into carry");
    assert!(cpu.p.z, "shifted zero should set zero");
    assert!(!cpu.p.n, "shifted zero should clear negative");
    assert_eq!(cpu.pc, 0x0201);
    assert_eq!(cpu.cycles, 2);
}

#[test]
fn cpu_step_executes_rol_accumulator_and_rotates_old_carry_in() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x80;
    cpu.p.c = true;
    bus.cpu_write(0x0200, 0x2A);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.a, 0x01);
    assert!(cpu.p.c, "bit 7 should move into carry");
    assert!(!cpu.p.z);
    assert!(!cpu.p.n);
    assert_eq!(cpu.pc, 0x0201);
    assert_eq!(cpu.cycles, 2);
}

#[test]
fn cpu_step_executes_lsr_zero_page_and_writes_result_back_to_memory() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    bus.cpu_write(0x0200, 0x46);
    bus.cpu_write(0x0201, 0x10);
    bus.cpu_write(0x0010, 0x01);

    cpu.cpu_step(&mut bus);

    assert_eq!(bus.cpu_read(0x0010), 0x00);
    assert!(cpu.p.c, "bit 0 should move into carry");
    assert!(cpu.p.z, "shifted zero should set zero");
    assert!(!cpu.p.n);
    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.cycles, 5);
}

#[test]
fn cpu_step_executes_ror_absolute_and_rotates_old_carry_into_bit_seven() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.p.c = true;
    bus.cpu_write(0x0200, 0x6E);
    bus.write_u16(0x0201, 0x1234);
    bus.cpu_write(0x1234, 0x01);

    cpu.cpu_step(&mut bus);

    assert_eq!(bus.cpu_read(0x1234), 0x80);
    assert!(cpu.p.c, "bit 0 should move into carry");
    assert!(!cpu.p.z);
    assert!(cpu.p.n, "carry-in should rotate into bit 7");
    assert_eq!(cpu.pc, 0x0203);
    assert_eq!(cpu.cycles, 6);
}

#[test]
fn cpu_step_executes_adc_immediate_and_sets_overflow_for_signed_addition() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x50;
    cpu.p.c = false;
    bus.cpu_write(0x0200, 0x69);
    bus.cpu_write(0x0201, 0x50);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.a, 0xA0);
    assert!(!cpu.p.c, "0x50 + 0x50 should not set carry");
    assert!(!cpu.p.z);
    assert!(cpu.p.n, "0xA0 should set negative");
    assert!(cpu.p.v, "positive + positive -> negative should set overflow");
    assert_eq!(cpu.cycles, 2);
}

#[test]
fn cpu_step_executes_adc_absolute_x_and_adds_cycle_when_page_is_crossed() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x01;
    cpu.x = 0x01;
    cpu.p.c = false;
    bus.cpu_write(0x0200, 0x7D);
    bus.write_u16(0x0201, 0x12FF);
    bus.cpu_write(0x1300, 0x01);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.a, 0x02);
    assert_eq!(cpu.pc, 0x0203);
    assert_eq!(cpu.cycles, 5);
}

#[test]
fn cpu_step_executes_sbc_immediate_without_borrow() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x10;
    cpu.p.c = true;
    bus.cpu_write(0x0200, 0xE9);
    bus.cpu_write(0x0201, 0x01);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.a, 0x0F);
    assert!(cpu.p.c, "no borrow should keep carry set");
    assert!(!cpu.p.z);
    assert!(!cpu.p.n);
    assert!(!cpu.p.v);
    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.cycles, 2);
}

#[test]
fn cpu_step_executes_sbc_immediate_and_clears_carry_when_borrow_occurs() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x00;
    cpu.p.c = true;
    bus.cpu_write(0x0200, 0xE9);
    bus.cpu_write(0x0201, 0x01);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.a, 0xFF);
    assert!(!cpu.p.c, "borrow should clear carry");
    assert!(!cpu.p.z);
    assert!(cpu.p.n, "0xFF should set negative");
    assert!(!cpu.p.v);
    assert_eq!(cpu.cycles, 2);
}

#[test]
fn cpu_step_executes_sbc_immediate_and_sets_overflow_for_signed_subtraction() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x80;
    cpu.p.c = true;
    bus.cpu_write(0x0200, 0xE9);
    bus.cpu_write(0x0201, 0x01);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.a, 0x7F);
    assert!(cpu.p.c);
    assert!(!cpu.p.z);
    assert!(!cpu.p.n);
    assert!(cpu.p.v, "negative - positive -> positive should set overflow");
    assert_eq!(cpu.cycles, 2);
}

#[test]
fn cpu_step_executes_sbc_indirect_y_and_adds_cycle_when_page_is_crossed() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x05;
    cpu.y = 0x01;
    cpu.p.c = true;
    bus.cpu_write(0x0200, 0xF1);
    bus.cpu_write(0x0201, 0x80);
    bus.cpu_write(0x0080, 0xFF);
    bus.cpu_write(0x0081, 0x12);
    bus.cpu_write(0x1300, 0x01);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.a, 0x04);
    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.cycles, 6);
}

#[test]
fn cpu_step_executes_beq_without_branch_when_zero_flag_is_clear() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.p.z = false;
    bus.cpu_write(0x0200, 0xF0);
    bus.cpu_write(0x0201, 0x05);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.cycles, 2);
}

#[test]
fn cpu_step_executes_beq_and_adds_cycle_when_branch_is_taken() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.p.z = true;
    bus.cpu_write(0x0200, 0xF0);
    bus.cpu_write(0x0201, 0x05);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.pc, 0x0207);
    assert_eq!(cpu.cycles, 3);
}

#[test]
fn cpu_step_executes_beq_and_adds_two_cycles_when_branch_crosses_page() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x20FD;
    cpu.p.z = true;
    bus.cpu_write(0x20FD, 0xF0);
    bus.cpu_write(0x20FE, 0x02);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.pc, 0x2101);
    assert_eq!(cpu.cycles, 4);
}

#[test]
fn cpu_step_executes_bne_when_zero_flag_is_clear() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.p.z = false;
    bus.cpu_write(0x0200, 0xD0);
    bus.cpu_write(0x0201, 0x05);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.pc, 0x0207);
    assert_eq!(cpu.cycles, 3);
}

#[test]
fn cpu_step_does_not_branch_on_bne_when_zero_flag_is_set() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.p.z = true;
    bus.cpu_write(0x0200, 0xD0);
    bus.cpu_write(0x0201, 0x05);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.cycles, 2);
}

#[test]
fn cpu_step_executes_jsr_absolute_and_pushes_return_address_to_stack() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x80FE;
    cpu.sp = 0xFD;
    bus.cpu_write(0x80FE, 0x20);
    bus.write_u16(0x80FF, 0x3456);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.pc, 0x3456);
    assert_eq!(bus.cpu_read(0x01FD), 0x81);
    assert_eq!(bus.cpu_read(0x01FC), 0x00);
    assert_eq!(cpu.sp, 0xFB);
    assert_eq!(cpu.cycles, 6);
    assert_eq!(cpu.clocks, 1);
}

#[test]
fn cpu_step_executes_rts_and_restores_pc_from_stack() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.sp = 0xFB;
    bus.cpu_write(0x0200, 0x60);
    bus.cpu_write(0x01FC, 0x34);
    bus.cpu_write(0x01FD, 0x12);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.pc, 0x1235);
    assert_eq!(cpu.sp, 0xFD);
    assert_eq!(cpu.cycles, 6);
    assert_eq!(cpu.clocks, 1);
}

#[test]
fn cpu_step_executes_jmp_absolute() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    bus.cpu_write(0x0200, 0x4C);
    bus.write_u16(0x0201, 0x3456);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.pc, 0x3456);
    assert_eq!(cpu.cycles, 3);
    assert_eq!(cpu.clocks, 1);
}

#[test]
fn cpu_step_executes_nop_zero_page_and_consumes_operand_byte() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x3C;
    cpu.x = 0x12;
    cpu.y = 0x34;
    bus.cpu_write(0x0200, 0x04);
    bus.cpu_write(0x0201, 0x99);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.pc, 0x0202);
    assert_eq!(cpu.a, 0x3C);
    assert_eq!(cpu.x, 0x12);
    assert_eq!(cpu.y, 0x34);
    assert_eq!(cpu.cycles, 3);
    assert_eq!(cpu.clocks, 1);
}

#[test]
fn cpu_step_unimplemented_opcode_with_absolute_mode_still_advances_pc() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    cpu.a = 0x11;
    cpu.x = 0x22;
    cpu.y = 0x33;
    bus.cpu_write(0x0200, 0x0E);
    bus.write_u16(0x0201, 0x4567);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.pc, 0x0203);
    assert_eq!(cpu.a, 0x11);
    assert_eq!(cpu.x, 0x22);
    assert_eq!(cpu.y, 0x33);
    assert_eq!(cpu.cycles, 6);
    assert_eq!(cpu.clocks, 1);
}

#[test]
fn cpu_step_executes_jmp_indirect_using_wrapped_high_byte() {
    let mut cpu = CPU::new();
    let mut bus = TestBus::new();
    cpu.pc = 0x0200;
    bus.cpu_write(0x0200, 0x6C);
    bus.write_u16(0x0201, 0x12FF);
    bus.cpu_write(0x12FF, 0x34);
    bus.cpu_write(0x1200, 0x12);
    bus.cpu_write(0x1300, 0x99);

    cpu.cpu_step(&mut bus);

    assert_eq!(cpu.pc, 0x1234);
    assert_eq!(cpu.cycles, 5);
    assert_eq!(cpu.clocks, 1);
}
