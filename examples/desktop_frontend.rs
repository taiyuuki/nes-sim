use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};
use nes_core::video::frame_to_argb32;
use nes_core::{ControllerButton, ControllerState, FrontendInput, FrontendRuntime, RunMode};
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

fn usage(program: &str) {
    eprintln!("Usage: {program} <rom-path>");
    eprintln!(r#"Example: {program} "roms/mmc1/Rockman2(J).nes""#);
    eprintln!("Controls:");
    eprintln!("  Arrows  D-pad");
    eprintln!("  X/Z     A/B");
    eprintln!("  Enter   Start");
    eprintln!("  Tab     Select");
    eprintln!("  P       Pause/Resume");
    eprintln!("  N       Step frame");
    eprintln!("  M       Step CPU instruction");
    eprintln!("  R       Reset");
    eprintln!("  F5      Save state");
    eprintln!("  F8      Load state");
    eprintln!("  Esc     Quit");
}

fn main() -> ExitCode {
    let mut args = env::args();
    let program = args
        .next()
        .unwrap_or_else(|| "desktop_frontend".to_string());

    let Some(rom_path) = args.next() else {
        usage(&program);
        return ExitCode::from(2);
    };
    if rom_path == "--help" || rom_path == "-h" {
        usage(&program);
        return ExitCode::SUCCESS;
    }

    let rom = match std::fs::read(&rom_path) {
        Ok(rom) => rom,
        Err(error) => {
            eprintln!("failed to read ROM {rom_path:?}: {error}");
            return ExitCode::from(1);
        }
    };

    let mut runtime = match FrontendRuntime::from_rom_bytes(&rom) {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("failed to load ROM {rom_path:?}: {error}");
            return ExitCode::from(1);
        }
    };
    let save_path = default_save_path(&rom_path);

    let mut window = match Window::new(
        "nes_core",
        nes_core::FRAME_WIDTH,
        nes_core::FRAME_HEIGHT,
        WindowOptions {
            resize: false,
            scale: Scale::X4,
            ..WindowOptions::default()
        },
    ) {
        Ok(window) => window,
        Err(error) => {
            eprintln!("failed to open window: {error}");
            return ExitCode::from(1);
        }
    };
    window.set_target_fps(60);

    let mut frames_in_window = 0u32;
    let mut fps = 0.0f32;
    let mut fps_window_start = Instant::now();
    let mut status_message = format!("save slot {}", save_path.display());

    while window.is_open() && !window.is_key_down(Key::Escape) {
        if window.is_key_pressed(Key::F5, KeyRepeat::No) {
            status_message = match runtime.save_state() {
                Ok(bytes) => match std::fs::write(&save_path, bytes) {
                    Ok(()) => format!("saved {}", save_path.display()),
                    Err(error) => format!("save failed: {error}"),
                },
                Err(error) => format!("save failed: {error}"),
            };
        }

        if window.is_key_pressed(Key::F8, KeyRepeat::No) {
            status_message = match std::fs::read(&save_path) {
                Ok(bytes) => match runtime.load_state(&bytes) {
                    Ok(()) => format!("loaded {}", save_path.display()),
                    Err(error) => format!("load failed: {error}"),
                },
                Err(error) => format!("load failed: {error}"),
            };
        }

        let input = collect_input(&window);
        let snapshot = runtime.step(input);
        if snapshot.status.quit_requested {
            break;
        }

        let buffer = frame_to_argb32(snapshot.video);
        if let Err(error) =
            window.update_with_buffer(&buffer, snapshot.video.width, snapshot.video.height)
        {
            eprintln!("failed to present frame: {error}");
            return ExitCode::from(1);
        }

        frames_in_window += 1;
        let elapsed = fps_window_start.elapsed();
        if elapsed >= Duration::from_secs(1) {
            fps = frames_in_window as f32 / elapsed.as_secs_f32();
            frames_in_window = 0;
            fps_window_start = Instant::now();
        }

        update_window_title(&mut window, &snapshot, fps, &status_message);
    }

    ExitCode::SUCCESS
}

fn collect_input(window: &Window) -> FrontendInput {
    let mut controller1 = ControllerState::new();
    set_button(
        &mut controller1,
        window.is_key_down(Key::X),
        ControllerButton::A,
    );
    set_button(
        &mut controller1,
        window.is_key_down(Key::Z),
        ControllerButton::B,
    );
    set_button(
        &mut controller1,
        window.is_key_down(Key::Enter),
        ControllerButton::Start,
    );
    set_button(
        &mut controller1,
        window.is_key_down(Key::Tab),
        ControllerButton::Select,
    );
    set_button(
        &mut controller1,
        window.is_key_down(Key::Up),
        ControllerButton::Up,
    );
    set_button(
        &mut controller1,
        window.is_key_down(Key::Down),
        ControllerButton::Down,
    );
    set_button(
        &mut controller1,
        window.is_key_down(Key::Left),
        ControllerButton::Left,
    );
    set_button(
        &mut controller1,
        window.is_key_down(Key::Right),
        ControllerButton::Right,
    );

    FrontendInput {
        controller1,
        reset: window.is_key_pressed(Key::R, KeyRepeat::No),
        toggle_pause: window.is_key_pressed(Key::P, KeyRepeat::No),
        step_frame: window.is_key_pressed(Key::N, KeyRepeat::No),
        step_cpu_instruction: window.is_key_pressed(Key::M, KeyRepeat::No),
        quit: window.is_key_pressed(Key::Escape, KeyRepeat::No),
        ..FrontendInput::default()
    }
}

fn set_button(state: &mut ControllerState, pressed: bool, button: ControllerButton) {
    state.set_pressed(button, pressed);
}

fn default_save_path(rom_path: &str) -> PathBuf {
    PathBuf::from(rom_path).with_extension("state")
}

fn update_window_title(
    window: &mut Window,
    snapshot: &nes_core::RuntimeSnapshot<'_>,
    fps: f32,
    status_message: &str,
) {
    let mode = match snapshot.status.mode {
        RunMode::Running => "running",
        RunMode::Paused => "paused",
    };
    let title = format!(
        "nes_core | {} | fps {:.1} | frame {} | pc {:04X} | cpu clocks {} | {}",
        mode,
        fps,
        snapshot.debug.ppu.frame,
        snapshot.debug.cpu.pc,
        snapshot.debug.cpu.clocks,
        status_message
    );
    window.set_title(&title);
}
