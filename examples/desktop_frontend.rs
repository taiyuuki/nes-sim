use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};
use nes_core::video::frame_to_argb32;
use nes_core::{ControllerButton, ControllerState, FrontendInput, FrontendRuntime, RunMode};
use std::collections::VecDeque;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
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
    let audio_player = match AudioPlayer::new(runtime.snapshot().audio.sample_rate) {
        Ok(player) => Some(player),
        Err(error) => {
            eprintln!("audio disabled: {error}");
            None
        }
    };

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

        if let Some(player) = &audio_player {
            player.push_samples(snapshot.audio.samples, snapshot.audio.sample_rate);
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

struct AudioPlayer {
    target_sample_rate: u32,
    device_sample_rate: u32,
    queue: Arc<Mutex<VecDeque<f32>>>,
    _stream: cpal::Stream,
}

impl AudioPlayer {
    fn new(target_sample_rate: u32) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default audio output device".to_string())?;
        let default_config = device
            .default_output_config()
            .map_err(|error| format!("failed to query default output config: {error}"))?;
        let channels = usize::from(default_config.channels());
        let device_sample_rate = default_config.sample_rate().0;
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let queue_for_stream = Arc::clone(&queue);
        let error_callback = |error| eprintln!("audio stream error: {error}");

        let stream = match default_config.sample_format() {
            cpal::SampleFormat::F32 => device
                .build_output_stream(
                    &default_config.config(),
                    move |data: &mut [f32], _| write_audio_data(data, channels, &queue_for_stream),
                    error_callback,
                    None,
                )
                .map_err(|error| format!("failed to build f32 audio stream: {error}"))?,
            cpal::SampleFormat::I16 => device
                .build_output_stream(
                    &default_config.config(),
                    move |data: &mut [i16], _| {
                        write_audio_data_i16(data, channels, &queue_for_stream)
                    },
                    error_callback,
                    None,
                )
                .map_err(|error| format!("failed to build i16 audio stream: {error}"))?,
            cpal::SampleFormat::U16 => device
                .build_output_stream(
                    &default_config.config(),
                    move |data: &mut [u16], _| {
                        write_audio_data_u16(data, channels, &queue_for_stream)
                    },
                    error_callback,
                    None,
                )
                .map_err(|error| format!("failed to build u16 audio stream: {error}"))?,
            sample_format => {
                return Err(format!(
                    "unsupported audio sample format: {sample_format:?}"
                ));
            }
        };
        stream
            .play()
            .map_err(|error| format!("failed to start audio stream: {error}"))?;

        Ok(Self {
            target_sample_rate,
            device_sample_rate,
            queue,
            _stream: stream,
        })
    }

    fn push_samples(&self, samples: &[f32], source_sample_rate: u32) {
        if samples.is_empty() {
            return;
        }

        let mono_samples = if source_sample_rate == self.device_sample_rate {
            samples.to_vec()
        } else {
            resample_mono(samples, source_sample_rate, self.device_sample_rate)
        };

        if let Ok(mut queue) = self.queue.lock() {
            for sample in mono_samples {
                queue.push_back(sample.clamp(-1.0, 1.0));
            }
            let max_samples = (self.target_sample_rate as usize).saturating_mul(2);
            while queue.len() > max_samples {
                let _ = queue.pop_front();
            }
        }
    }
}

fn write_audio_data(output: &mut [f32], channels: usize, queue: &Arc<Mutex<VecDeque<f32>>>) {
    let mut next_sample = 0.0;
    if let Ok(mut queue) = queue.lock() {
        for frame in output.chunks_mut(channels) {
            next_sample = queue.pop_front().unwrap_or(0.0);
            for sample in frame {
                *sample = next_sample;
            }
        }
    } else {
        for sample in output.iter_mut() {
            *sample = next_sample;
        }
    }
}

fn write_audio_data_i16(output: &mut [i16], channels: usize, queue: &Arc<Mutex<VecDeque<f32>>>) {
    let mut mono = vec![0.0; output.len()];
    write_audio_data(&mut mono, channels, queue);
    for (dst, src) in output.iter_mut().zip(mono) {
        *dst = (src * f32::from(i16::MAX)) as i16;
    }
}

fn write_audio_data_u16(output: &mut [u16], channels: usize, queue: &Arc<Mutex<VecDeque<f32>>>) {
    let mut mono = vec![0.0; output.len()];
    write_audio_data(&mut mono, channels, queue);
    for (dst, src) in output.iter_mut().zip(mono) {
        let normalized = (src * 0.5 + 0.5).clamp(0.0, 1.0);
        *dst = (normalized * f32::from(u16::MAX)) as u16;
    }
}

fn resample_mono(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if samples.is_empty() || source_rate == 0 || target_rate == 0 {
        return Vec::new();
    }
    if source_rate == target_rate {
        return samples.to_vec();
    }

    let target_len = samples.len().saturating_mul(target_rate as usize) / source_rate as usize;
    if target_len == 0 {
        return Vec::new();
    }

    let step = source_rate as f32 / target_rate as f32;
    let mut pos = 0.0f32;
    let mut resampled = Vec::with_capacity(target_len);
    for _ in 0..target_len {
        let index = pos.floor() as usize;
        let frac = pos - index as f32;
        let a = samples[index.min(samples.len() - 1)];
        let b = samples[(index + 1).min(samples.len() - 1)];
        resampled.push(a + (b - a) * frac);
        pos += step;
    }
    resampled
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
