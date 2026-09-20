use nes_sim::NES;
use nes_sim::headless::write_frame_ppm;
use nes_sim::is_fds_image;
use std::env;
use std::path::Path;
use std::process::ExitCode;

fn usage(program: &str) {
    eprintln!("Usage: {program} <fds-path> [bios-path] [frames] [output-ppm]");
    eprintln!(
        r#"Example: {program} "roms/fds/Super Mario Bros. (Japan).fds" "roms/fds/BIOS Files/DISKSYS.ROM" 600 out/smb.ppm"#
    );
}

fn main() -> ExitCode {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "fds_boot".to_string());

    let Some(rom_path) = args.next() else {
        usage(&program);
        return ExitCode::from(2);
    };
    let bios_path = args
        .next()
        .unwrap_or_else(|| "roms/fds/BIOS Files/DISKSYS.ROM".to_string());
    let frames = args
        .next()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(600);
    let output_path = args
        .next()
        .unwrap_or_else(|| "out/fds_boot.ppm".to_string());

    let rom = match std::fs::read(&rom_path) {
        Ok(rom) => rom,
        Err(error) => {
            eprintln!("failed to read disk image {rom_path:?}: {error}");
            return ExitCode::from(1);
        }
    };
    if !is_fds_image(&rom) {
        eprintln!("{rom_path:?} is not an FDS disk image");
        return ExitCode::from(1);
    }

    let bios = match std::fs::read(&bios_path) {
        Ok(bios) => bios,
        Err(error) => {
            eprintln!("failed to read FDS BIOS {bios_path:?}: {error}");
            return ExitCode::from(1);
        }
    };

    let mut nes = NES::new();
    if let Err(error) = nes.load_cartridge_fds(&rom, &bios) {
        eprintln!("failed to load FDS image {rom_path:?}: {error}");
        return ExitCode::from(1);
    }
    nes.reset();

    if let Some(info) = nes.fds_info() {
        eprintln!(
            "FDS: {} side(s), side {} selected, inserted: {}",
            info.side_count, info.selected_side, info.inserted
        );
    }

    for _ in 0..frames {
        nes.run_frame();
    }

    if let Some(parent) = Path::new(&output_path).parent()
        && !parent.as_os_str().is_empty()
        && let Err(error) = std::fs::create_dir_all(parent)
    {
        eprintln!("failed to create output directory {:?}: {}", parent, error);
        return ExitCode::from(1);
    }
    if let Err(error) = write_frame_ppm(&output_path, nes.video_frame()) {
        eprintln!("failed to write {output_path:?}: {error}");
        return ExitCode::from(1);
    }

    let pixels = nes.frame_pixels();
    let hash: u64 = pixels.iter().enumerate().fold(0u64, |acc, (i, &p)| {
        acc.wrapping_mul(31).wrapping_add(p as u64 + i as u64)
    });
    println!("frame {}: hash {hash:#018x}", nes.frame_number());
    println!("wrote {output_path}");
    ExitCode::SUCCESS
}
