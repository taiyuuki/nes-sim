use super::*;
use crate::bus::{CPUBus, NESBus};

fn clock_cpu_cycles(cpu: &mut CPU, bus: &mut NESBus, cycles: usize) {
    for _ in 0..cycles {
        cpu.clock(bus);
        bus.advance_dma_cpu_phase();
    }
}

fn make_ines(prg_banks: u8, chr_banks: u8, flags6: u8) -> Vec<u8> {
    let mut rom = vec![0; 16];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = prg_banks;
    rom[5] = chr_banks;
    rom[6] = flags6;

    let prg_len = prg_banks as usize * 0x4000;
    let chr_len = chr_banks as usize * 0x2000;
    rom.extend((0..prg_len).map(|index| (index & 0xFF) as u8));
    rom.extend((0..chr_len).map(|index| (0x80 | (index & 0x7F)) as u8));
    rom
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

    clock_cpu_cycles(&mut cpu, &mut bus, 1);
    clock_cpu_cycles(&mut cpu, &mut bus, 2);

    clock_cpu_cycles(&mut cpu, &mut bus, 1);
    assert!(bus.dma_in_progress(), "writing $4014 should queue OAM DMA");
    assert_eq!(cpu.pc, 0x0005, "STA should finish before DMA takes over");

    clock_cpu_cycles(&mut cpu, &mut bus, 1);
    assert_eq!(cpu.pc, 0x0005, "DMA should halt opcode fetch while active");

    while bus.dma_in_progress() {
        clock_cpu_cycles(&mut cpu, &mut bus, 1);
    }

    assert_eq!(bus.ppu().oam_byte(0x00), 0x00);
    assert_eq!(bus.ppu().oam_byte(0x80), 0x80);
    assert_eq!(bus.ppu().oam_byte(0xFF), 0xFF);
    assert_eq!(
        bus.ppu().oam_addr(),
        0x00,
        "256 OAM writes should wrap address"
    );

    clock_cpu_cycles(&mut cpu, &mut bus, 1);
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
        clock_cpu_cycles(&mut cpu, &mut bus, 1);
        cycles += 1;
    }

    assert_eq!(cycles, 514);
}

#[test]
fn oam_dma_consumes_513_cycles_when_started_on_put_phase() {
    let mut cpu = CPU::new();
    let mut bus = NESBus::new();

    bus.advance_dma_cpu_phase();
    bus.cpu_write(0x4014, 0x02);
    assert!(bus.dma_in_progress());

    let mut cycles = 0;
    while bus.dma_in_progress() {
        clock_cpu_cycles(&mut cpu, &mut bus, 1);
        cycles += 1;
    }

    assert_eq!(cycles, 513);
}

#[test]
fn dmc_dma_fetches_sample_and_updates_open_bus() {
    let mut cpu = CPU::new();
    let mut bus = NESBus::new();
    let rom = make_ines(1, 1, 0x00);

    bus.load_cartridge_ines(&rom).expect("NROM should load");
    bus.cpu_write(0x4012, 0x01);
    bus.cpu_write(0x4013, 0x00);
    bus.cpu_write(0x4015, 0x10);

    clock_cpu_cycles(&mut cpu, &mut bus, 3);

    assert_eq!(bus.cpu_read(0x4000), 0x40);
    assert_eq!(bus.cpu_read(0x4015) & 0x10, 0x00);
}

#[test]
fn dmc_load_dma_completes_in_three_cycles_on_get_phase() {
    let mut cpu = CPU::new();
    let mut bus = NESBus::new();
    let rom = make_ines(1, 1, 0x00);

    bus.load_cartridge_ines(&rom).expect("NROM should load");
    bus.cpu_write(0x4012, 0x01);
    bus.cpu_write(0x4013, 0x00);
    bus.cpu_write(0x4015, 0x10);

    clock_cpu_cycles(&mut cpu, &mut bus, 2);
    assert_eq!(bus.cpu_read(0x4015) & 0x10, 0x10);

    clock_cpu_cycles(&mut cpu, &mut bus, 1);
    assert_eq!(bus.cpu_read(0x4015) & 0x10, 0x00);
}

#[test]
fn dmc_load_dma_completes_in_four_cycles_on_put_phase() {
    let mut cpu = CPU::new();
    let mut bus = NESBus::new();
    let rom = make_ines(1, 1, 0x00);

    bus.load_cartridge_ines(&rom).expect("NROM should load");
    bus.advance_dma_cpu_phase();
    bus.cpu_write(0x4012, 0x01);
    bus.cpu_write(0x4013, 0x00);
    bus.cpu_write(0x4015, 0x10);

    clock_cpu_cycles(&mut cpu, &mut bus, 3);
    assert_eq!(bus.cpu_read(0x4015) & 0x10, 0x10);

    clock_cpu_cycles(&mut cpu, &mut bus, 1);
    assert_eq!(bus.cpu_read(0x4015) & 0x10, 0x00);
}
