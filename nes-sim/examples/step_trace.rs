use nes_sim::{ControllerButton, ControllerState, NES};
use std::process::ExitCode;

// 快速单步轨迹：先跑到指定帧（可按键），再单步N条指令打印PC/寄存器
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("Usage: step_trace <rom> <warmup-frames> <steps> [press-from] [press-interval]");
        return ExitCode::from(2);
    }
    let rom = std::fs::read(&args[0]).expect("read rom");
    let warmup: u64 = args[1].parse().unwrap_or(150);
    let steps: usize = args[2].parse().unwrap_or(800);
    let press_from: u64 = args
        .get(3)
        .map(|v| v.parse().unwrap_or(u64::MAX))
        .unwrap_or(u64::MAX);
    let interval: u64 = args.get(4).map(|v| v.parse().unwrap_or(120)).unwrap_or(120);

    let mut nes = NES::new();
    nes.load_cartridge_ines(&rom).expect("load rom");
    nes.reset();

    let released = ControllerState::new();
    let mut pressed = ControllerState::new();
    pressed.set_pressed(ControllerButton::A, true);

    for frame in 0..warmup {
        let pressing = frame >= press_from && (frame - press_from) % interval < 15;
        nes.set_controller_state(0, if pressing { pressed } else { released });
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
