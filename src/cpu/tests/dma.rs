use super::*;
use crate::bus::{CPUBus, NESBus};

fn clock_cpu_cycles(cpu: &mut CPU, bus: &mut NESBus, cycles: usize) {
    for _ in 0..cycles {
        cpu.cpu_clock(bus);
    }
}

#[test]
fn oam_dma_request_stalls_cpu_until_transfer_completes() {
    let mut cpu = CPU::new();
    let mut bus = NESBus::new();

    cpu.pc = 0x0000;
    bus.cpu_write(0x0000, 0xA9); // LDA #$03
    bus.cpu_write(0x0001, 0x03);
    bus.cpu_write(0x0002, 0x8D); // STA $4014
    bus.cpu_write(0x0003, 0x14);
    bus.cpu_write(0x0004, 0x40);
    bus.cpu_write(0x0005, 0xA9); // LDA #$99
    bus.cpu_write(0x0006, 0x99);

    for i in 0..=0xFFu16 {
        bus.cpu_write(0x0300 | i, i as u8);
    }

    cpu.cpu_clock(&mut bus);
    clock_cpu_cycles(&mut cpu, &mut bus, 2);

    cpu.cpu_clock(&mut bus);
    assert!(bus.dma_in_progress(), "writing $4014 should queue OAM DMA");
    assert_eq!(cpu.pc, 0x0005, "STA should finish before DMA takes over");

    cpu.cpu_clock(&mut bus);
    assert_eq!(cpu.pc, 0x0005, "DMA should halt opcode fetch while active");

    while bus.dma_in_progress() {
        cpu.cpu_clock(&mut bus);
    }

    assert_eq!(bus.ppu().oam_byte(0x00), 0x00);
    assert_eq!(bus.ppu().oam_byte(0x80), 0x80);
    assert_eq!(bus.ppu().oam_byte(0xFF), 0xFF);
    assert_eq!(
        bus.ppu().oam_addr(),
        0x00,
        "256 OAM writes should wrap address"
    );

    cpu.cpu_clock(&mut bus);
    assert_eq!(cpu.a, 0x99, "CPU should resume with the next instruction");
    assert_eq!(cpu.pc, 0x0007);
}

#[test]
fn oam_dma_consumes_514_cycles_when_started_on_get_phase() {
    let mut cpu = CPU::new();
    let mut bus = NESBus::new();

    bus.cpu_write(0x4014, 0x02);
    assert!(bus.dma_in_progress());

    let mut cycles = 0;
    while bus.dma_in_progress() {
        cpu.cpu_clock(&mut bus);
        cycles += 1;
    }

    assert_eq!(cycles, 514);
}

#[test]
fn oam_dma_consumes_513_cycles_when_started_on_put_phase() {
    let mut cpu = CPU::new();
    let mut bus = NESBus::new();

    assert!(
        !bus.try_dma(),
        "an idle DMA tick should only flip CPU slot phase"
    );
    bus.cpu_write(0x4014, 0x02);
    assert!(bus.dma_in_progress());

    let mut cycles = 0;
    while bus.dma_in_progress() {
        cpu.cpu_clock(&mut bus);
        cycles += 1;
    }

    assert_eq!(cycles, 513);
}
