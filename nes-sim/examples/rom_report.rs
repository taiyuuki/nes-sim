use nes_sim::NES;
use nes_sim::video::frame_to_argb32;
use std::collections::HashMap;
use std::process::ExitCode;

fn analyze_frame(frame: &[u32]) -> (usize, f64) {
    let mut counts: HashMap<[u8; 3], usize> = HashMap::new();
    for &px in frame {
        *counts
            .entry([(px >> 16) as u8, (px >> 8) as u8, px as u8])
            .or_insert(0) += 1;
    }
    let total = frame.len().max(1);
    let max = counts.values().copied().max().unwrap_or(0);
    (counts.len(), max as f64 / total as f64)
}

fn run_rom(path: &str, frames_a: usize, frames_b: usize) -> Result<(), String> {
    let rom = std::fs::read(path).map_err(|e| format!("read error: {e}"))?;
    let mut nes = NES::new();
    nes.load_cartridge_ines(&rom)
        .map_err(|e| format!("load error: {e}"))?;
    nes.reset();

    for _ in 0..frames_a {
        nes.run_frame();
    }
    let frame_a = frame_to_argb32(nes.video_frame());
    let (colors_a, bg_ratio_a) = analyze_frame(&frame_a);

    for _ in 0..(frames_b - frames_a) {
        nes.run_frame();
    }
    let frame_b = frame_to_argb32(nes.video_frame());
    let (colors_b, bg_ratio_b) = analyze_frame(&frame_b);
    let changed = frame_a != frame_b;

    println!(
        "{path}\n  frame {frames_a}: colors={colors_a} bg_ratio={bg_ratio_a:.3} | frame {frames_b}: colors={colors_b} bg_ratio={bg_ratio_b:.3} changed={changed}"
    );
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Usage: rom_report <rom> [rom...]");
        return ExitCode::from(2);
    }
    for rom in &args {
        if let Err(error) = run_rom(rom, 240, 600) {
            println!("{rom}\n  FAILED: {error}");
        }
    }
    ExitCode::SUCCESS
}
