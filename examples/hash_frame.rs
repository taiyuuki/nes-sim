use nes_sim::NES;
use nes_sim::headless::{frame_to_ppm, stable_byte_hash};
use std::env;
use std::path::Path;
use std::process::ExitCode;

fn usage(program: &str) {
    eprintln!("Usage: {program} <rom-path> [frames] [output-ppm]");
    eprintln!(r#"Example: {program} "roms/mmc1/Rockman2(J).nes" 180 "out/rockman2-current.ppm""#);
}

fn main() -> ExitCode {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "hash_frame".to_string());

    let Some(rom_path) = args.next() else {
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
        None => 180,
    };
    let output_path = args.next();

    let rom = match std::fs::read(&rom_path) {
        Ok(rom) => rom,
        Err(error) => {
            eprintln!("failed to read ROM {rom_path:?}: {error}");
            return ExitCode::from(1);
        }
    };

    let mut nes = NES::new();
    if let Err(error) = nes.load_cartridge_ines(&rom) {
        eprintln!("failed to load ROM {rom_path:?}: {error}");
        return ExitCode::from(1);
    }
    nes.reset();

    for _ in 0..frames {
        nes.run_frame();
    }

    let ppm = frame_to_ppm(nes.video_frame());
    let hash = stable_byte_hash(&ppm);
    println!(
        "{} frame={} hash=0x{:016X}",
        rom_path,
        nes.frame_number(),
        hash
    );

    if let Some(output_path) = output_path {
        if let Some(parent) = Path::new(&output_path).parent()
            && !parent.as_os_str().is_empty()
        {
            if let Err(error) = std::fs::create_dir_all(parent) {
                eprintln!("failed to create output directory {:?}: {}", parent, error);
                return ExitCode::from(1);
            }
        }

        if let Err(error) = std::fs::write(&output_path, &ppm) {
            eprintln!("failed to write PPM {output_path:?}: {error}");
            return ExitCode::from(1);
        }
        println!("wrote {}", output_path);
    }

    ExitCode::SUCCESS
}
