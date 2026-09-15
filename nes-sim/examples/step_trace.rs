use nes_sim::NES;
use std::process::ExitCode;

// 快速单步轨迹：先跑到指定帧，再单步N条指令打印PC/寄存器（不依赖debug feature）
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("Usage: step_trace <rom> <warmup-frames> <steps>");
        return ExitCode::from(2);
    }
    let rom = std::fs::read(&args[0]).expect("read rom");
    let warmup: u64 = args[1].parse().unwrap_or(150);
    let steps: usize = args[2].parse().unwrap_or(800);

    let mut nes = NES::new();
    nes.load_cartridge_ines(&rom).expect("load rom");
    nes.reset();

    for _ in 0..warmup {
        nes.run_frame();
    }
    println!("== after {} frames ==", warmup);
    for i in 0..steps {
        let cpu = nes.debug_snapshot().cpu;
        println!(
            "{:04}: pc={:04X} a={:02X} x={:02X} y={:02X} sp={:02X} p={:02X}",
            i, cpu.pc, cpu.a, cpu.x, cpu.y, cpu.sp, cpu.status
        );
        nes.step_cpu_instruction();
    }
    ExitCode::SUCCESS
}
