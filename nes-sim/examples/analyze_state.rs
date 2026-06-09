use nes_sim::NES;
use nes_sim::headless::{frame_to_ppm, stable_byte_hash};
use nes_sim::{ControllerButton, ControllerState};
use std::env;
use std::path::Path;
use std::process::ExitCode;

fn usage(program: &str) {
    eprintln!("Usage: {program} <rom-path> <state-path> [frames] [input-mode] [out-dir]");
    eprintln!(
        r#"Example: {program} "E:\roms\mapper_007\Time Lord\Time Lord (U) [!].nes" "E:\roms\mapper_007\Time Lord\Time Lord (U) [!].state" 600 right out/timelord-state"#
    );
    eprintln!("input-mode: none | right | right_a");
}

fn changed_pixels(a: &[u8], b: &[u8]) -> usize {
    a.iter().zip(b).filter(|(x, y)| x != y).count()
}

fn changed_pixels_region(a: &[u8], b: &[u8], y0: usize, y1: usize) -> usize {
    let width = 256usize;
    let start = y0 * width;
    let end = y1 * width;
    a[start..end]
        .iter()
        .zip(&b[start..end])
        .filter(|(x, y)| x != y)
        .count()
}

fn changed_bbox(a: &[u8], b: &[u8]) -> Option<(usize, usize, usize, usize)> {
    let width = 256usize;
    let height = 240usize;
    let mut min_x = width;
    let mut min_y = height;
    let mut max_x = 0usize;
    let mut max_y = 0usize;
    let mut any = false;

    for y in 0..height {
        for x in 0..width {
            let i = y * width + x;
            if a[i] != b[i] {
                any = true;
                if x < min_x {
                    min_x = x;
                }
                if y < min_y {
                    min_y = y;
                }
                if x > max_x {
                    max_x = x;
                }
                if y > max_y {
                    max_y = y;
                }
            }
        }
    }

    if any {
        Some((min_x, min_y, max_x, max_y))
    } else {
        None
    }
}

fn main() -> ExitCode {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "analyze_state".to_string());

    let Some(rom_path) = args.next() else {
        usage(&program);
        return ExitCode::from(2);
    };
    let Some(state_path) = args.next() else {
        usage(&program);
        return ExitCode::from(2);
    };
    let frames = match args.next() {
        Some(value) => match value.parse::<usize>() {
            Ok(frames) => frames,
            Err(error) => {
                eprintln!("invalid frame count {value:?}: {error}");
                return ExitCode::from(2);
            }
        },
        None => 240,
    };
    let input_mode = args.next().unwrap_or_else(|| "none".to_string());
    let out_dir = args.next();

    let rom = match std::fs::read(&rom_path) {
        Ok(rom) => rom,
        Err(error) => {
            eprintln!("failed to read ROM {rom_path:?}: {error}");
            return ExitCode::from(1);
        }
    };
    let state = match std::fs::read(&state_path) {
        Ok(state) => state,
        Err(error) => {
            eprintln!("failed to read state {state_path:?}: {error}");
            return ExitCode::from(1);
        }
    };

    let mut nes = NES::new();
    if let Err(error) = nes.load_cartridge_ines(&rom) {
        eprintln!("failed to load ROM {rom_path:?}: {error}");
        return ExitCode::from(1);
    }
    if let Err(error) = nes.load_state(&state) {
        eprintln!("failed to load state {state_path:?}: {error}");
        return ExitCode::from(1);
    }

    let mut prev = nes.frame_pixels().to_vec();
    let mut prev_hash = stable_byte_hash(&frame_to_ppm(nes.video_frame()));
    let mut same_hash_run = 0usize;
    println!(
        "start frame={} hash=0x{:016X}",
        nes.frame_number(),
        prev_hash
    );

    if let Some(ref out_dir) = out_dir {
        if !out_dir.is_empty() {
            let path = Path::new(out_dir);
            if let Err(error) = std::fs::create_dir_all(path) {
                eprintln!("failed to create output directory {:?}: {}", path, error);
                return ExitCode::from(1);
            }
        }
    }

    for i in 1..=frames {
        let mut controller = ControllerState::new();
        if input_mode == "right" || input_mode == "right_a" {
            controller.set_pressed(ControllerButton::Right, true);
        }
        if input_mode == "right_a" {
            controller.set_pressed(ControllerButton::A, true);
        }
        nes.set_controller_state(0, controller);
        nes.run_frame();
        let frame = nes.frame_pixels().to_vec();
        let hash = stable_byte_hash(&frame_to_ppm(nes.video_frame()));
        let changed_all = changed_pixels(&prev, &frame);
        let changed_top_hud = changed_pixels_region(&prev, &frame, 0, 48);
        let changed_bottom_hud = changed_pixels_region(&prev, &frame, 208, 240);
        let changed_gameplay = changed_pixels_region(&prev, &frame, 48, 208);
        let debug = nes.debug_snapshot();

        if hash == prev_hash {
            same_hash_run += 1;
        } else {
            same_hash_run = 0;
        }

        if let Some((min_x, min_y, max_x, max_y)) = changed_bbox(&prev, &frame) {
            println!(
                "i={} frame={} hash=0x{:016X} changed_all={} changed_top_hud={} changed_bottom_hud={} changed_gameplay={} bbox=({},{})->({},{}) same_hash_run={} pc={:04X} ppu_scanline={} in_vblank={}",
                i,
                nes.frame_number(),
                hash,
                changed_all,
                changed_top_hud,
                changed_bottom_hud,
                changed_gameplay,
                min_x,
                min_y,
                max_x,
                max_y,
                same_hash_run,
                debug.cpu.pc,
                debug.ppu.scanline,
                debug.ppu.in_vblank
            );
        } else {
            println!(
                "i={} frame={} hash=0x{:016X} changed_all=0 changed_top_hud=0 changed_bottom_hud=0 changed_gameplay=0 bbox=none same_hash_run={} pc={:04X} ppu_scanline={} in_vblank={}",
                i,
                nes.frame_number(),
                hash,
                same_hash_run,
                debug.cpu.pc,
                debug.ppu.scanline,
                debug.ppu.in_vblank
            );
        }

        if let Some(ref out_dir) = out_dir
            && !out_dir.is_empty()
            && (i <= 8 || same_hash_run >= 30 || i % 60 == 0)
        {
            let ppm = frame_to_ppm(nes.video_frame());
            let output = Path::new(out_dir).join(format!("frame_{:04}.ppm", i));
            if let Err(error) = std::fs::write(&output, ppm) {
                eprintln!("failed to write {:?}: {}", output, error);
                return ExitCode::from(1);
            }
        }

        prev = frame;
        prev_hash = hash;
    }

    ExitCode::SUCCESS
}
