use nes_sim::NES;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Usage: dbg_rom <rom> [frames...]");
        return ExitCode::from(2);
    }
    let rom = std::fs::read(&args[0]).expect("read rom");
    let mut nes = NES::new();
    nes.load_cartridge_ines(&rom).expect("load rom");
    nes.reset();

    let mut checkpoints: Vec<usize> = args[1..].iter().filter_map(|f| f.parse().ok()).collect();
    if checkpoints.is_empty() {
        checkpoints = vec![10, 60, 240, 600];
    }
    checkpoints.sort_unstable();

    let mut next = 0;
    let sample_mode = std::env::var_os("DBG_SAMPLE").is_some();
    if sample_mode {
        let target = *checkpoints.last().unwrap_or(&600);
        for frame in 0..target {
            nes.run_frame();
            if frame
                % std::env::var("DBG_EVERY")
                    .ok()
                    .and_then(|v| v.parse::<usize>().ok())
                    .unwrap_or(60)
                == 0
            {
                let snap = nes.debug_snapshot();
                println!(
                    "f{:>4} pc={:04X} a={:02X} x={:02X} y={:02X}",
                    frame, snap.cpu.pc, snap.cpu.a, snap.cpu.x, snap.cpu.y
                );
            }
        }
        return ExitCode::SUCCESS;
    }
    for frame in 0..=*checkpoints.last().unwrap_or(&0) {
        while next < checkpoints.len() && checkpoints[next] == frame {
            let snap = nes.debug_snapshot();
            println!(
                "frame {:>5}: pc={:04X} a={:02X} x={:02X} y={:02X} sp={:02X} instr={} irq_pending={} nmi={}",
                frame,
                snap.cpu.pc,
                snap.cpu.a,
                snap.cpu.x,
                snap.cpu.y,
                snap.cpu.sp,
                snap.cpu.instruction_counter,
                snap.cpu.irq_pending,
                snap.cpu.nmi_line
            );
            #[cfg(feature = "debug")]
            {
                let dis = nes.debug_disassemble(8);
                for row in dis.instructions.iter().take(4) {
                    println!(
                        "    {:04X}: {:02X}{:02X}{:02X} {} {}",
                        row.address,
                        row.bytes[0],
                        row.bytes[1],
                        row.bytes[2],
                        row.mnemonic,
                        row.operand
                    );
                }
            }
            next += 1;
        }
        nes.run_frame();
    }
    {
        let cpu = nes.debug_snapshot().cpu;
        println!(
            "--- final state: pc={:04X} a={:02X} x={:02X} y={:02X} sp={:02X} status={:02X} (I={}) ppu_frame={} irq_line={} ---",
            cpu.pc,
            cpu.a,
            cpu.x,
            cpu.y,
            cpu.sp,
            cpu.status,
            (cpu.status >> 2) & 1,
            nes.debug_snapshot().ppu.frame,
            cpu.irq_pending
        );
    }
    #[cfg(feature = "debug")]
    if std::env::var_os("DBG_MEM").is_some() {
        let mem = nes.debug_memory_snapshot();
        println!("zp 00-3F: {}", dump(&mem.ram[0x00..0x40]));
        println!("zp 40-7F: {}", dump(&mem.ram[0x40..0x80]));
        println!("stack: {}", dump(&mem.ram[0x100..0x200]));
    }
    ExitCode::SUCCESS
}
