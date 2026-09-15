use nes_sim::{Breakpoint, NES};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        eprintln!("Usage: trace_bp <rom> <bp-addr-hex> [max-hits] [max-frames]");
        return ExitCode::from(2);
    }
    let rom = std::fs::read(&args[0]).expect("read rom");
    let bp_addr = u16::from_str_radix(args[1].trim_start_matches("0x"), 16).expect("bp addr");
    let max_hits: usize = args.get(2).map(|v| v.parse().unwrap_or(10)).unwrap_or(10);
    let max_frames: u64 = args.get(3).map(|v| v.parse().unwrap_or(900)).unwrap_or(900);

    let mut nes = NES::new();
    nes.load_cartridge_ines(&rom).expect("load rom");
    nes.reset();
    nes.add_breakpoint(Breakpoint::Address(bp_addr));

    let mut hits = 0;
    let start_frame = nes.frame_number();
    let max_clocks: u64 = max_frames * 90_000;
    let mut clocks = 0u64;
    let min_frame: u64 = std::env::var("DBG_MINFRAME")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let every: usize = std::env::var("DBG_EVERY_HIT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let mut printed = 0usize;
    while clocks < max_clocks && printed < max_hits {
        nes.clock();
        clocks += 1;
        if nes.breakpoint_hit().is_some() {
            let frame_now = nes.frame_number() - start_frame;
            if frame_now < min_frame || (hits % every) != 0 {
                hits += 1;
                nes.set_paused(false);
                continue;
            }
            hits += 1;
            printed += 1;
            let cpu = nes.debug_snapshot().cpu;
            let mem = nes.debug_memory_snapshot();
            let sp = cpu.sp as usize;
            println!(
                "hit#{} frame={} pc={:04X} a={:02X} x={:02X} y={:02X} sp={:02X} status={:02X}",
                hits,
                nes.frame_number() - start_frame,
                cpu.pc,
                cpu.a,
                cpu.x,
                cpu.y,
                cpu.sp,
                cpu.status
            );
            // dump the last 16 stack bytes above sp (return addresses live there)
            let top = &mem.ram[0x100 + sp + 1..0x100 + sp + 1 + 16];
            println!(
                "  stack[sp+1..+16]: {}",
                top.iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            // candidate return addresses (little endian pairs)
            let pairs: Vec<String> = (0..top.len() - 1)
                .step_by(2)
                .map(|i| format!("{:04X}", top[i] as u16 | (top[i + 1] as u16) << 8))
                .collect();
            println!("  ret candidates: {}", pairs.join(" "));
            println!(
                "  zp 00-0F: {}",
                mem.ram[0x00..0x10]
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            println!(
                "  token ptr ($06/$07) = {:04X}",
                mem.ram[0x06] as u16 | (mem.ram[0x07] as u16) << 8
            );
            nes.set_paused(false);
        }
    }
    println!("done: hits={} clocks={}", hits, clocks);
    ExitCode::SUCCESS
}
