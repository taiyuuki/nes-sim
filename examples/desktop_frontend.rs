use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};
use nes_sim::video::{VideoBuffer, frame_to_argb32_into};
use nes_sim::{
    ControllerButton, ControllerState, FrontendInput, FrontendRuntime, RunMode, TVSystem,
};
use std::collections::VecDeque;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const AUDIO_TARGET_BUFFER_MS: usize = 15;
const AUDIO_MAX_BUFFER_MS: usize = 100;
const AUDIO_CATCHUP_MAX_FRAMES: usize = 2;

/// Windows 高精度定时器守卫，离开作用域时自动恢复
#[cfg(target_os = "windows")]
struct PrecisionTimerGuard;

#[cfg(target_os = "windows")]
impl PrecisionTimerGuard {
    fn new() -> Self {
        unsafe {
            winmm::timeBeginPeriod(1);
        }
        Self
    }
}

#[cfg(target_os = "windows")]
impl Drop for PrecisionTimerGuard {
    fn drop(&mut self) {
        unsafe {
            winmm::timeEndPeriod(1);
        }
    }
}

#[cfg(target_os = "windows")]
mod winmm {
    use std::ffi::c_uint;
    unsafe extern "system" {
        pub fn timeBeginPeriod(uPeriod: c_uint) -> c_uint;
        pub fn timeEndPeriod(uPeriod: c_uint) -> c_uint;
    }
}

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
    eprintln!("  1..5    Toggle APU mute (P1/P2/TRI/NOI/DMC)");
    eprintln!("  0       Clear APU mute");
    eprintln!("  F5      Save state");
    eprintln!("  F8      Load state");
    eprintln!("  Esc     Quit");
}

fn main() -> ExitCode {
    // Windows 高精度定时器
    #[cfg(target_os = "windows")]
    let _timer_guard = PrecisionTimerGuard::new();

    let mut args = env::args();
    let program = args
        .next()
        .unwrap_or_else(|| "desktop_frontend".to_string());

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
                if parse_tv_system_override(&value).is_none() {
                    eprintln!("invalid --tv-system value {value:?}");
                    usage(&program);
                    return ExitCode::from(2);
                }
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

    let mut runtime = match FrontendRuntime::from_rom_bytes(&rom) {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("failed to load ROM {rom_path:?}: {error}");
            return ExitCode::from(1);
        }
    };
    let save_path = default_save_path(&rom_path);
    let mut audio_player = match AudioPlayer::new(runtime.snapshot().audio.sample_rate) {
        Ok(player) => Some(player),
        Err(error) => {
            eprintln!("audio disabled: {error}");
            None
        }
    };
    if let Some(player) = &mut audio_player {
        runtime
            .nes_mut()
            .set_apu_sample_rate(player.output_sample_rate());
        eprintln!(
            "Audio: {}Hz, target buffer {}ms ({} samples), max {}ms ({} samples), catch-up {} frames",
            player.output_sample_rate(),
            AUDIO_TARGET_BUFFER_MS,
            player.target_queue_samples,
            AUDIO_MAX_BUFFER_MS,
            player.max_queue_samples,
            AUDIO_CATCHUP_MAX_FRAMES,
        );

        // 立即启动音频播放
        player.start_playback();
        eprintln!(
            "Audio playback started, queue: {} samples",
            player.queue_len()
        );
    }

    let mut window = match Window::new(
        "nes_sim",
        nes_sim::FRAME_WIDTH,
        nes_sim::FRAME_HEIGHT,
        WindowOptions {
            resize: false,
            scale: Scale::X2,
            ..WindowOptions::default()
        },
    ) {
        Ok(window) => window,
        Err(error) => {
            eprintln!("failed to open window: {error}");
            return ExitCode::from(1);
        }
    };
    let mut frames_in_window = 0u32;
    let mut fps = 0.0f32;
    let mut fps_window_start = Instant::now();
    let mut status_message = format!("save slot {}", save_path.display());
    let mut apu_mute_mask = runtime.nes().apu_debug_mute_mask();

    let mut snapshot;

    // 预分配视频缓冲区，避免每帧堆分配
    let mut video_buffer = VideoBuffer::new(nes_sim::FRAME_WIDTH * nes_sim::FRAME_HEIGHT);

    let frame_period = Duration::from_micros(16_667);
    let mut next_frame_deadline = Instant::now() + frame_period;
    let mut frame_times = VecDeque::new();
    let frame_time_window = 60; // 采样 60 帧计算平均值

    // 每秒帧时间分解统计
    let mut step_time_acc = Duration::ZERO;
    let mut catchup_time_acc = Duration::ZERO;
    let mut render_time_acc = Duration::ZERO;
    // let mut frame_count_acc = 0u32;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let frame_start = Instant::now();
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

        if handle_apu_debug_hotkeys(&window, &mut runtime, &mut apu_mute_mask) {
            status_message = format!("apu mute {}", apu_mute_mask_to_string(apu_mute_mask));
        }

        let input = collect_input(&window);
        let t_step = Instant::now();
        snapshot = runtime.step(input);
        let step_elapsed = t_step.elapsed();
        step_time_acc += step_elapsed;
        if snapshot.status.quit_requested {
            break;
        }

        let t_catchup = Instant::now();
        if let Some(player) = &mut audio_player {
            player.push_samples(snapshot.audio.samples, snapshot.audio.sample_rate);

            // 如果还未开始播放（例如从暂停恢复），启动播放
            if !player.playback_started && matches!(snapshot.status.mode, RunMode::Running) {
                player.start_playback();
            }

            if matches!(snapshot.status.mode, RunMode::Running) {
                let catch_up_input = FrontendInput {
                    controller1: input.controller1,
                    controller2: input.controller2,
                    ..FrontendInput::default()
                };

                let mut catch_up_frames = 0;
                while player.is_queue_low() && catch_up_frames < AUDIO_CATCHUP_MAX_FRAMES {
                    snapshot = runtime.step(catch_up_input);
                    if snapshot.status.quit_requested {
                        break;
                    }
                    player.push_samples(snapshot.audio.samples, snapshot.audio.sample_rate);
                    catch_up_frames += 1;
                }
            }
        }
        catchup_time_acc += t_catchup.elapsed();

        if snapshot.status.quit_requested {
            break;
        }

        // 使用预分配的缓冲区转换帧数据
        let t_render = Instant::now();
        frame_to_argb32_into(snapshot.video, video_buffer.as_mut_slice());
        if let Err(error) = window.update_with_buffer(
            video_buffer.as_slice(),
            snapshot.video.width,
            snapshot.video.height,
        ) {
            eprintln!("failed to present frame: {error}");
            return ExitCode::from(1);
        }
        render_time_acc += t_render.elapsed();

        // 帧时间统计（包含 catch-up 以反映真实 CPU 占用）
        let frame_elapsed = frame_start.elapsed();
        frame_times.push_back(frame_elapsed);
        if frame_times.len() > frame_time_window {
            frame_times.pop_front();
        }

        // 帧率控制：当模拟器无法维持 60fps 时不添加额外延迟
        let now = Instant::now();
        if frame_elapsed < frame_period {
            // 模拟器快于实时，等待下一个 deadline
            next_frame_deadline += frame_period;
            if now < next_frame_deadline {
                std::thread::sleep(next_frame_deadline - now);
            } else {
                next_frame_deadline = now + frame_period;
            }
        } else {
            // 模拟器慢于实时，重置 deadline 避免累积滞后
            next_frame_deadline = now + frame_period;
        }

        frames_in_window += 1;
        // frame_count_acc += 1;
        let elapsed = fps_window_start.elapsed();
        if elapsed >= Duration::from_secs(1) {
            fps = frames_in_window as f32 / elapsed.as_secs_f32();
            frames_in_window = 0;
            fps_window_start = Instant::now();

            // if let Some(player) = &audio_player {
            //     if let Some((count, samples)) = player.underrun_stats() {
            //         if count > 0 {
            //             eprintln!(
            //                 "AUDIO UNDERRUN: {} callbacks, {} samples dropped ({} fps)",
            //                 count, samples, fps
            //             );
            //         }
            //     }
            // }
            // if frame_count_acc > 0 {
            //     let n = frame_count_acc as f32;
            //     let queue_info = audio_player
            // .as_ref()
            // .map_or(String::new(), |p| format!(" queue={}", p.queue_len()));
            // eprintln!(
            //     "Frame timing: step={:.2}ms catchup={:.2}ms render={:.2}ms total={:.2}ms ({} frames){}",
            //     step_time_acc.as_secs_f32() * 1000.0 / n,
            //     catchup_time_acc.as_secs_f32() * 1000.0 / n,
            //     render_time_acc.as_secs_f32() * 1000.0 / n,
            //     (step_time_acc + catchup_time_acc + render_time_acc).as_secs_f32() * 1000.0 / n,
            //     frame_count_acc,
            //     queue_info,
            // );
            // }
            step_time_acc = Duration::ZERO;
            catchup_time_acc = Duration::ZERO;
            render_time_acc = Duration::ZERO;
            // frame_count_acc = 0;
        }

        // 计算平均帧时间
        let avg_frame_time = if !frame_times.is_empty() {
            let sum: Duration = frame_times.iter().sum();
            sum / frame_times.len() as u32
        } else {
            Duration::ZERO
        };

        update_window_title(
            &mut window,
            &snapshot,
            fps,
            &status_message,
            apu_mute_mask,
            avg_frame_time,
        );
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
    output_sample_rate: u32,
    target_queue_samples: usize,
    max_queue_samples: usize,
    resampler: Mutex<StreamingLinearResampler>,
    output_state: Arc<Mutex<AudioOutputState>>,
    stream: cpal::Stream,
    playback_started: bool,
}

struct AudioOutputState {
    queue: VecDeque<f32>,
    last_sample: f32,
    underrun_count: u64,
    underrun_samples: u64,
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
        let target_queue_samples = device_sample_rate as usize * AUDIO_TARGET_BUFFER_MS / 1000;
        let max_queue_samples = device_sample_rate as usize * AUDIO_MAX_BUFFER_MS / 1000;
        let resampler = Mutex::new(StreamingLinearResampler::new(device_sample_rate));
        let output_state = Arc::new(Mutex::new(AudioOutputState {
            queue: VecDeque::new(),
            last_sample: 0.0,
            underrun_count: 0,
            underrun_samples: 0,
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
        Ok(Self {
            output_sample_rate: device_sample_rate,
            target_queue_samples,
            max_queue_samples,
            resampler,
            output_state,
            stream,
            playback_started: false,
        })
    }

    fn output_sample_rate(&self) -> u32 {
        self.output_sample_rate
    }

    fn start_playback(&mut self) {
        if !self.playback_started {
            if let Err(e) = self.stream.play() {
                eprintln!("failed to start audio stream: {e}");
            }
            self.playback_started = true;
        }
    }

    fn queue_len(&self) -> usize {
        self.output_state.lock().map_or(0, |s| s.queue.len())
    }

    fn push_samples(&self, samples: &[f32], source_sample_rate: u32) {
        if samples.is_empty() {
            return;
        }

        if let Ok(mut output_state) = self.output_state.lock() {
            if source_sample_rate == self.output_sample_rate {
                for &sample in samples {
                    output_state.queue.push_back(sample.clamp(-1.0, 1.0));
                }
            } else if let Ok(mut resampler) = self.resampler.lock() {
                let resampled = resampler.resample_chunk(samples, source_sample_rate);
                for sample in resampled {
                    output_state.queue.push_back(sample.clamp(-1.0, 1.0));
                }
            }

            while output_state.queue.len() > self.max_queue_samples {
                output_state.queue.pop_front();
            }
        }
    }

    fn is_queue_low(&self) -> bool {
        if let Ok(output_state) = self.output_state.lock() {
            output_state.queue.len() < self.target_queue_samples
        } else {
            false
        }
    }

    // fn underrun_stats(&self) -> Option<(u64, u64)> {
    //     if let Ok(mut output_state) = self.output_state.lock() {
    //         let stats = (output_state.underrun_count, output_state.underrun_samples);
    //         output_state.underrun_count = 0;
    //         output_state.underrun_samples = 0;
    //         Some(stats)
    //     } else {
    //         None
    //     }
    // }
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
    const UNDERRUN_DECAY: f32 = 0.98;
    const SILENCE_EPSILON: f32 = 1e-4;
    let mut next_sample = 0.0;
    if let Ok(mut output_state) = output_state.lock() {
        for frame in output.chunks_mut(channels) {
            next_sample = if let Some(sample) = output_state.queue.pop_front() {
                sample
            } else {
                output_state.underrun_count += 1;
                output_state.underrun_samples += 1;
                let decayed = output_state.last_sample * UNDERRUN_DECAY;
                if decayed.abs() < SILENCE_EPSILON {
                    0.0
                } else {
                    decayed
                }
            };
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
    fn audio_callback_decays_to_silence_on_underrun() {
        let output_state = Arc::new(Mutex::new(AudioOutputState {
            queue: VecDeque::from([0.25]),
            last_sample: -0.5,
            underrun_count: 0,
            underrun_samples: 0,
        }));
        let mut output = [0.0f32; 4];

        write_audio_data(&mut output, 1, &output_state);

        assert_eq!(output[0], 0.25);
        assert!(output[1] < 0.25 && output[1] > 0.0);
        assert!(output[2] < output[1]);
        assert!(output[3] < output[2]);
    }
}

fn handle_apu_debug_hotkeys(
    window: &Window,
    runtime: &mut FrontendRuntime,
    apu_mute_mask: &mut u8,
) -> bool {
    let mut updated = false;

    let mut toggle = |key: Key, bit: u8| {
        if window.is_key_pressed(key, KeyRepeat::No) {
            *apu_mute_mask ^= bit;
            updated = true;
        }
    };

    toggle(Key::Key1, 0x01);
    toggle(Key::Key2, 0x02);
    toggle(Key::Key3, 0x04);
    toggle(Key::Key4, 0x08);
    toggle(Key::Key5, 0x10);

    if window.is_key_pressed(Key::Key0, KeyRepeat::No) {
        *apu_mute_mask = 0;
        updated = true;
    }

    if updated {
        runtime.nes_mut().set_apu_debug_mute_mask(*apu_mute_mask);
    }

    updated
}

fn apu_mute_mask_to_string(mask: u8) -> String {
    let mut parts = Vec::new();
    if (mask & 0x01) != 0 {
        parts.push("P1");
    }
    if (mask & 0x02) != 0 {
        parts.push("P2");
    }
    if (mask & 0x04) != 0 {
        parts.push("TRI");
    }
    if (mask & 0x08) != 0 {
        parts.push("NOI");
    }
    if (mask & 0x10) != 0 {
        parts.push("DMC");
    }

    if parts.is_empty() {
        "none".to_string()
    } else {
        parts.join("+")
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
    snapshot: &nes_sim::RuntimeSnapshot<'_>,
    fps: f32,
    status_message: &str,
    apu_mute_mask: u8,
    avg_frame_time: Duration,
) {
    let mode = match snapshot.status.mode {
        RunMode::Running => "running",
        RunMode::Paused => "paused",
    };
    let frame_time_ms = avg_frame_time.as_secs_f64() * 1000.0;
    let title = format!(
        "nes_sim | {} | fps {:.1} | frame {} | pc {:04X} | cpu clocks {} | mute {} | {:.1}ms/frame | {}",
        mode,
        fps,
        snapshot.debug.ppu.frame,
        snapshot.debug.cpu.pc,
        snapshot.debug.cpu.clocks,
        apu_mute_mask_to_string(apu_mute_mask),
        frame_time_ms,
        status_message
    );
    window.set_title(&title);
}
