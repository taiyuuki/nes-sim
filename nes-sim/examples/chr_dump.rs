use nes_sim::{ControllerButton, ControllerState, NES};
use std::process::ExitCode;

// 跑到指定帧后dump CHR-RAM内容分布
fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rom = std::fs::read(&args[0]).expect("read rom");
    let press_at: u64 = args.get(1).map(|v| v.parse().unwrap_or(900)).unwrap_or(900);
    let dump_at: u64 = args
        .get(2)
        .map(|v| v.parse().unwrap_or(2000))
        .unwrap_or(2000);

    let mut nes = NES::new();
    nes.load_cartridge_ines(&rom).expect("load rom");
    nes.reset();

    let released = ControllerState::new();
    let mut pressed = ControllerState::new();
    pressed.set_pressed(ControllerButton::A, true);

    for frame in 0..dump_at {
        let pressing = frame >= press_at && (frame - press_at) % 120 < 15;
        nes.set_controller_state(0, if pressing { pressed } else { released });
        nes.run_frame();
    }

    #[cfg(feature = "debug")]
    {
        let mem = nes.debug_memory_snapshot();
        println!("palette: {:02X?}", mem.palette);
    }
    let chr = nes.debug_read_chr();
    // 每1K块统计非零字节
    for block in 0..8 {
        let start = block * 0x400;
        let nonzero = chr[start..start + 0x400]
            .iter()
            .filter(|&&b| b != 0)
            .count();
        println!(
            "chr[{:04X}-{:04X}]: nonzero={}/1024",
            start,
            start + 0x3FF,
            nonzero
        );
    }
    // 前256字节样本
    println!("chr[0000..]: {:02X?}", &chr[0x00..0x20]);
    println!("chr[1000..]: {:02X?}", &chr[0x1000..0x1020]);
    println!("chr[1800..]: {:02X?}", &chr[0x1800..0x1820]);
    ExitCode::SUCCESS
}
