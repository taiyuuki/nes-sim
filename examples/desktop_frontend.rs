use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};
use nes_core::video::frame_to_argb32;
use nes_core::{
    ControllerButton, ControllerState, FrontendInput, FrontendRuntime, RunMode, TVSystem,
};
use std::collections::VecDeque;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const AUDIO_TARGET_BUFFER_MS: usize = 50;
const AUDIO_MAX_BUFFER_MS: usize = 100;
const AUDIO_CRITICAL_BUFFER_MS: usize = 12;
const AUDIO_CATCH_UP_FRAME_LIMIT: usize = 1;

fn usage(program: &str) {
    eprintln!("Usage: {program} [--tv-system auto|ntsc|pal|dendy] <rom-path>");
    eprintln!(r#"Example: {program} --tv-system ntsc "roms/mmc1/Rockman2(J).nes""#);
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

    let mut tv_system_override = Some(TVSystem::NTSC);
    let mut rom_path = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => {
                usage(&program);
                return ExitCode::SUCCESS;
            }
            "--tv-system" => {
                let Some(value) = args.next() else {
                    eprintln!("missing value after --tv-system");
                    usage(&program);
                    return ExitCode::from(2);
                };
                tv_system_override = match parse_tv_system_override(&value) {
                    Some(value) => value,
                    None => {
                        eprintln!("invalid --tv-system value {value:?}");
                        usage(&program);
                        return ExitCode::from(2);
                    }
                };
            }
            _ if rom_path.is_none() => rom_path = Some(arg),
            _ => {
                eprintln!("unexpected argument {arg:?}");
                usage(&program);
                return ExitCode::from(2);
            }
        }
    }

    let Some(rom_path) = rom_path else {
        usage(&program);
        return ExitCode::from(2);
    };

    let rom = match std::fs::read(&rom_path) {
        Ok(rom) => rom,
        Err(error) => {
            eprintln!("failed to read ROM {rom_path:?}: {error}");
            return ExitCode::from(1);
        }
    };

    let mut runtime =
        match FrontendRuntime::from_rom_bytes_with_tv_system_override(&rom, tv_system_override) {
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

    let mut snapshot = runtime.snapshot();

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
        snapshot = runtime.step(input);
        if snapshot.status.quit_requested {
            break;
        }

        if let Some(player) = &audio_player {
            player.push_samples(snapshot.audio.samples, snapshot.audio.sample_rate);

            let allow_audio_catch_up = matches!(snapshot.status.mode, RunMode::Running)
                && !input.reset
                && !input.toggle_pause
                && !input.step_frame
                && !input.step_cpu_instruction
                && !input.quit;

            if allow_audio_catch_up {
                let catch_up_input = FrontendInput {
                    controller1: input.controller1,
                    controller2: input.controller2,
                    ..FrontendInput::default()
                };

                let mut catch_up_frames = 0usize;
                while player.needs_urgent_refill() && catch_up_frames < AUDIO_CATCH_UP_FRAME_LIMIT {
                    snapshot = runtime.step(catch_up_input);
                    if snapshot.status.quit_requested {
                        break;
                    }
                    player.push_samples(snapshot.audio.samples, snapshot.audio.sample_rate);
                    catch_up_frames += 1;
                }
            }
        }

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

fn parse_tv_system_override(value: &str) -> Option<Option<TVSystem>> {
    match value {
        "auto" => Some(None),
        "ntsc" => Some(Some(TVSystem::NTSC)),
        "pal" => Some(Some(TVSystem::PAL)),
        "dendy" => Some(Some(TVSystem::DENDY)),
        _ => None,
    }
}

struct AudioPlayer {
    critical_queue_samples: usize,
    target_queue_samples: usize,
    max_queue_samples: usize,
    resampler: Mutex<StreamingLinearResampler>,
    output_state: Arc<Mutex<AudioOutputState>>,
    _stream: cpal::Stream,
}

struct AudioOutputState {
    queue: VecDeque<f32>,
    last_sample: f32,
}

impl AudioPlayer {
    fn new(_target_sample_rate: u32) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default audio output device".to_string())?;
        let default_config = device
            .default_output_config()
            .map_err(|error| format!("failed to query default output config: {error}"))?;
        let channels = usize::from(default_config.channels());
        let device_sample_rate = default_config.sample_rate().0;
        let critical_queue_samples = device_sample_rate as usize * AUDIO_CRITICAL_BUFFER_MS / 1000;
        let target_queue_samples = device_sample_rate as usize * AUDIO_TARGET_BUFFER_MS / 1000;
        let max_queue_samples = device_sample_rate as usize * AUDIO_MAX_BUFFER_MS / 1000;
        let resampler = Mutex::new(StreamingLinearResampler::new(device_sample_rate));
        let output_state = Arc::new(Mutex::new(AudioOutputState {
            queue: VecDeque::new(),
            last_sample: 0.0,
        }));
        let output_state_for_stream = Arc::clone(&output_state);
        let error_callback = |error| eprintln!("audio stream error: {error}");

        let stream = match default_config.sample_format() {
            cpal::SampleFormat::F32 => device
                .build_output_stream(
                    &default_config.config(),
                    move |data: &mut [f32], _| {
                        write_audio_data(data, channels, &output_state_for_stream)
                    },
                    error_callback,
                    None,
                )
                .map_err(|error| format!("failed to build f32 audio stream: {error}"))?,
            cpal::SampleFormat::I16 => device
                .build_output_stream(
                    &default_config.config(),
                    move |data: &mut [i16], _| {
                        write_audio_data_i16(data, channels, &output_state_for_stream)
                    },
                    error_callback,
                    None,
                )
                .map_err(|error| format!("failed to build i16 audio stream: {error}"))?,
            cpal::SampleFormat::U16 => device
                .build_output_stream(
                    &default_config.config(),
                    move |data: &mut [u16], _| {
                        write_audio_data_u16(data, channels, &output_state_for_stream)
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
            critical_queue_samples,
            target_queue_samples,
            max_queue_samples,
            resampler,
            output_state,
            _stream: stream,
        })
    }

    fn push_samples(&self, samples: &[f32], source_sample_rate: u32) {
        if samples.is_empty() {
            return;
        }

        let mono_samples = if let Ok(mut resampler) = self.resampler.lock() {
            resampler.resample_chunk(samples, source_sample_rate)
        } else {
            return;
        };

        if let Ok(mut output_state) = self.output_state.lock() {
            for sample in mono_samples {
                output_state.queue.push_back(sample.clamp(-1.0, 1.0));
            }

            if output_state.queue.len() > self.max_queue_samples {
                while output_state.queue.len() > self.target_queue_samples {
                    if let Some(sample) = output_state.queue.pop_front() {
                        output_state.last_sample = sample;
                    }
                }
            }

            while output_state.queue.len() > self.max_queue_samples {
                if let Some(sample) = output_state.queue.pop_front() {
                    output_state.last_sample = sample;
                }
            }
        }
    }

    fn needs_urgent_refill(&self) -> bool {
        if let Ok(output_state) = self.output_state.lock() {
            output_state.queue.len() < self.critical_queue_samples
        } else {
            false
        }
    }
}

struct StreamingLinearResampler {
    target_rate: u32,
    source_rate: u32,
    step: f64,
    history: VecDeque<f32>,
    history_start_index: i64,
    latest_input_index: i64,
    next_output_position: f64,
}

impl StreamingLinearResampler {
    fn new(target_rate: u32) -> Self {
        Self {
            target_rate,
            source_rate: 0,
            step: 1.0,
            history: VecDeque::new(),
            history_start_index: 0,
            latest_input_index: -1,
            next_output_position: 0.0,
        }
    }

    fn reset(&mut self, source_rate: u32) {
        self.source_rate = source_rate;
        self.step = source_rate as f64 / self.target_rate as f64;
        self.history.clear();
        self.history_start_index = 0;
        self.latest_input_index = -1;
        self.next_output_position = 0.0;
    }

    fn resample_chunk(&mut self, samples: &[f32], source_rate: u32) -> Vec<f32> {
        if samples.is_empty() || source_rate == 0 || self.target_rate == 0 {
            return Vec::new();
        }

        if self.source_rate != source_rate || self.history.is_empty() {
            self.reset(source_rate);
        }

        for &sample in samples {
            self.latest_input_index += 1;
            self.history.push_back(sample);
        }

        let mut output = Vec::new();
        while self.can_emit_sample() {
            output.push(self.current_output_sample());
            self.next_output_position += self.step;
            self.discard_consumed_history();
        }

        output
    }

    fn can_emit_sample(&self) -> bool {
        self.latest_input_index >= self.next_output_position.floor() as i64 + 1
    }

    fn current_output_sample(&self) -> f32 {
        let index = self.next_output_position.floor() as i64;
        let frac = (self.next_output_position - index as f64) as f32;
        let a = self.history[(index - self.history_start_index) as usize];
        let b = self.history[(index + 1 - self.history_start_index) as usize];
        a + (b - a) * frac
    }

    fn discard_consumed_history(&mut self) {
        let keep_from = self.next_output_position.floor() as i64;
        while self.history_start_index < keep_from {
            let _ = self.history.pop_front();
            self.history_start_index += 1;
        }
    }
}

fn write_audio_data(
    output: &mut [f32],
    channels: usize,
    output_state: &Arc<Mutex<AudioOutputState>>,
) {
    let mut next_sample = 0.0;
    if let Ok(mut output_state) = output_state.lock() {
        for frame in output.chunks_mut(channels) {
            next_sample = output_state
                .queue
                .pop_front()
                .unwrap_or(output_state.last_sample);
            output_state.last_sample = next_sample;
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

fn write_audio_data_i16(
    output: &mut [i16],
    channels: usize,
    output_state: &Arc<Mutex<AudioOutputState>>,
) {
    let mut mono = vec![0.0; output.len()];
    write_audio_data(&mut mono, channels, output_state);
    for (dst, src) in output.iter_mut().zip(mono) {
        *dst = (src * f32::from(i16::MAX)) as i16;
    }
}

fn write_audio_data_u16(
    output: &mut [u16],
    channels: usize,
    output_state: &Arc<Mutex<AudioOutputState>>,
) {
    let mut mono = vec![0.0; output.len()];
    write_audio_data(&mut mono, channels, output_state);
    for (dst, src) in output.iter_mut().zip(mono) {
        let normalized = (src * 0.5 + 0.5).clamp(0.0, 1.0);
        *dst = (normalized * f32::from(u16::MAX)) as u16;
    }
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

#[cfg(test)]
mod tests {
    use super::{AudioOutputState, StreamingLinearResampler, write_audio_data};
    use std::collections::VecDeque;
    use std::f32::consts::PI;
    use std::sync::{Arc, Mutex};

    fn estimate_positive_zero_crossing_frequency(samples: &[f32], sample_rate: f32) -> f32 {
        let mut crossings = 0usize;
        for window in samples.windows(2) {
            if window[0] <= 0.0 && window[1] > 0.0 {
                crossings += 1;
            }
        }
        crossings as f32 * sample_rate / samples.len() as f32
    }

    #[test]
    fn streaming_resampler_preserves_tone_across_chunk_boundaries() {
        let source_rate = 44_100u32;
        let target_rate = 48_000u32;
        let tone_hz = 440.0f32;
        let phase_step = 2.0 * PI * tone_hz / source_rate as f32;
        let mut phase = 0.0f32;
        let mut resampler = StreamingLinearResampler::new(target_rate);
        let mut output = Vec::new();

        for _ in 0..120 {
            let mut chunk = Vec::with_capacity(367);
            for _ in 0..367 {
                chunk.push(phase.sin());
                phase += phase_step;
            }
            output.extend(resampler.resample_chunk(&chunk, source_rate));
        }

        let measured_hz =
            estimate_positive_zero_crossing_frequency(&output[1024..], target_rate as f32);
        assert!(
            (measured_hz - tone_hz).abs() < 3.0,
            "expected about {tone_hz:.2} Hz, measured {measured_hz:.2} Hz"
        );
    }

    #[test]
    fn audio_callback_reuses_last_sample_on_underrun() {
        let output_state = Arc::new(Mutex::new(AudioOutputState {
            queue: VecDeque::from([0.25]),
            last_sample: -0.5,
        }));
        let mut output = [0.0f32; 4];

        write_audio_data(&mut output, 1, &output_state);

        assert_eq!(output, [0.25, 0.25, 0.25, 0.25]);
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
