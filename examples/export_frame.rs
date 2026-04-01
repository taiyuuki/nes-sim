use nes_core::NES;
use nes_core::headless::write_frame_ppm;
use std::env;
use std::path::Path;
use std::process::ExitCode;

fn usage(program: &str) {
    eprintln!("Usage: {program} <rom-path> <output-ppm> [frames]");
    eprintln!(r#"Example: {program} "roms/mmc1/Rockman2(J).nes" "out/rockman2.ppm" 180"#);
}

fn main() -> ExitCode {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "export_frame".to_string());

    let Some(rom_path) = args.next() else {
        usage(&program);
        return ExitCode::from(2);
    };
    let Some(output_path) = args.next() else {
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
        None => 120,
    };

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

    if let Some(parent) = Path::new(&output_path).parent()
        && !parent.as_os_str().is_empty()
    {
        if let Err(error) = std::fs::create_dir_all(parent) {
            eprintln!("failed to create output directory {:?}: {}", parent, error);
            return ExitCode::from(1);
        }
    }

    if let Err(error) = write_frame_ppm(&output_path, nes.video_frame()) {
        eprintln!("failed to write PPM {output_path:?}: {error}");
        return ExitCode::from(1);
    }

    println!(
        "wrote frame {} from {} to {}",
        nes.frame_number(),
        rom_path,
        output_path
    );
    ExitCode::SUCCESS
}
