use nes_sim::{ControllerButton, ControllerState, NES};
use std::process::ExitCode;

// 跑warmup帧后按START若干帧再继续，输出PPM
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 4 {
        eprintln!(
            "Usage: press_start <rom> <warmup-frames> <hold-frames> <extra-frames> [out-ppm]"
        );
        return ExitCode::from(2);
    }
    let rom = std::fs::read(&args[0]).expect("read rom");
    let warmup: u64 = args[1].parse().unwrap_or(600);
    let hold: u64 = args[2].parse().unwrap_or(30);
    let extra: u64 = args[3].parse().unwrap_or(600);
    let out = args
        .get(4)
        .cloned()
        .unwrap_or_else(|| "/tmp/press_start.ppm".to_string());

    let mut nes = NES::new();
    nes.load_cartridge_ines(&rom).expect("load rom");
    nes.reset();

    let released = ControllerState::new();
    let mut pressed = ControllerState::new();
    pressed.set_pressed(ControllerButton::Start, true);

    for _ in 0..warmup {
        nes.run_frame();
    }
    for _ in 0..hold {
        nes.set_controller_state(0, pressed);
        nes.run_frame();
    }
    for _ in 0..extra {
        nes.set_controller_state(0, released);
        nes.run_frame();
    }
    nes_sim::headless::write_frame_ppm(&out, nes.video_frame()).expect("write ppm");
    println!("wrote {out}");
    ExitCode::SUCCESS
}
