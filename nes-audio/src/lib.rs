//! cpal 音频输出播放器：核心推样本进队列，cpal 回调消费。
//! 由 nes-desktop / nes-compat 共用（自 nes-desktop/src/main.rs 迁移）。

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const AUDIO_TARGET_BUFFER_MS: usize = 50;
pub const AUDIO_MAX_BUFFER_MS: usize = 200;

pub struct AudioPlayer {
    output_sample_rate: u32,
    target_queue_samples: usize,
    max_queue_samples: usize,
    resampler: Mutex<StreamingLinearResampler>,
    output_state: Arc<Mutex<AudioOutputState>>,
    stream: cpal::Stream,
    playback_started: bool,
}

pub struct AudioOutputState {
    queue: VecDeque<f32>,
    last_sample: f32,
    underrun_count: u64,
    underrun_samples: u64,
    underrun_last_report: Instant,
}

impl AudioPlayer {
    pub fn new(_target_sample_rate: u32) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| "no default audio output device".to_string())?;
        let default_config = device
            .default_output_config()
            .map_err(|error| format!("failed to query default output config: {error}"))?;
        let channels = usize::from(default_config.channels());
        let device_sample_rate = default_config.sample_rate();
        let target_queue_samples = device_sample_rate as usize * AUDIO_TARGET_BUFFER_MS / 1000;
        let max_queue_samples = device_sample_rate as usize * AUDIO_MAX_BUFFER_MS / 1000;
        let resampler = Mutex::new(StreamingLinearResampler::new(device_sample_rate));
        let output_state = Arc::new(Mutex::new(AudioOutputState {
            queue: VecDeque::new(),
            last_sample: 0.0,
            underrun_count: 0,
            underrun_samples: 0,
            underrun_last_report: Instant::now(),
        }));
        let output_state_for_stream = Arc::clone(&output_state);
        let error_callback = |error| eprintln!("audio stream error: {error}");

        let stream = match default_config.sample_format() {
            cpal::SampleFormat::F32 => device
                .build_output_stream(
                    default_config.config(),
                    move |data: &mut [f32], _| {
                        write_audio_data(data, channels, &output_state_for_stream)
                    },
                    error_callback,
                    None,
                )
                .map_err(|error| format!("failed to build f32 audio stream: {error}"))?,
            cpal::SampleFormat::I16 => device
                .build_output_stream(
                    default_config.config(),
                    move |data: &mut [i16], _| {
                        write_audio_data_i16(data, channels, &output_state_for_stream)
                    },
                    error_callback,
                    None,
                )
                .map_err(|error| format!("failed to build i16 audio stream: {error}"))?,
            cpal::SampleFormat::U16 => device
                .build_output_stream(
                    default_config.config(),
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

    pub fn output_sample_rate(&self) -> u32 {
        self.output_sample_rate
    }

    pub fn target_queue_len(&self) -> usize {
        self.target_queue_samples
    }

    pub fn max_queue_samples(&self) -> usize {
        self.max_queue_samples
    }

    pub fn playback_started(&self) -> bool {
        self.playback_started
    }

    pub fn start_playback(&mut self) {
        if !self.playback_started {
            if let Err(e) = self.stream.play() {
                eprintln!("failed to start audio stream: {e}");
            }
            self.playback_started = true;
        }
    }

    pub fn stop_playback(&mut self) {
        if self.playback_started {
            if let Err(e) = self.stream.pause() {
                eprintln!("failed to pause audio stream: {e}");
            }
            self.playback_started = false;
        }
        // 清空音频队列
        if let Ok(mut output_state) = self.output_state.lock() {
            output_state.queue.clear();
        }
    }

    pub fn queue_len(&self) -> usize {
        self.output_state.lock().map_or(0, |s| s.queue.len())
    }

    pub fn push_samples(&self, samples: &[f32], source_sample_rate: u32) {
        if samples.is_empty() {
            return;
        }

        // 静音阈值：低于此值的样本被截断为 0，避免浮点噪声
        const SILENCE_THRESHOLD: f32 = 1e-5;

        if let Ok(mut output_state) = self.output_state.lock() {
            if source_sample_rate == self.output_sample_rate {
                for &sample in samples {
                    let sample = sample.clamp(-1.0, 1.0);
                    output_state
                        .queue
                        .push_back(if sample.abs() < SILENCE_THRESHOLD {
                            0.0
                        } else {
                            sample
                        });
                }
            } else if let Ok(mut resampler) = self.resampler.lock() {
                let resampled = resampler.resample_chunk(samples, source_sample_rate);
                for sample in resampled {
                    let sample = sample.clamp(-1.0, 1.0);
                    output_state
                        .queue
                        .push_back(if sample.abs() < SILENCE_THRESHOLD {
                            0.0
                        } else {
                            sample
                        });
                }
            }

            while output_state.queue.len() > self.max_queue_samples {
                output_state.queue.pop_front();
            }
        }
    }

    pub fn is_queue_low(&self) -> bool {
        if let Ok(output_state) = self.output_state.lock() {
            output_state.queue.len() < self.target_queue_samples
        } else {
            false
        }
    }
}

pub struct StreamingLinearResampler {
    target_rate: u32,
    source_rate: u32,
    step: f64,
    history: VecDeque<f32>,
    history_start_index: i64,
    latest_input_index: i64,
    next_output_position: f64,
}

impl StreamingLinearResampler {
    pub fn new(target_rate: u32) -> Self {
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

    pub fn resample_chunk(&mut self, samples: &[f32], source_rate: u32) -> Vec<f32> {
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
        self.latest_input_index > self.next_output_position.floor() as i64
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
    // 更快的衰减，更快到达静音
    const UNDERRUN_DECAY: f32 = 0.85;
    const SILENCE_EPSILON: f32 = 1e-5;
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

        // 每秒打印一次 underrun 统计
        if output_state.underrun_count > 0
            && output_state.underrun_last_report.elapsed() >= Duration::from_secs(1)
        {
            eprintln!(
                "AUDIO UNDERRUN: {} callbacks, {} samples dropped, queue: {} samples",
                output_state.underrun_count,
                output_state.underrun_samples,
                output_state.queue.len()
            );
            output_state.underrun_count = 0;
            output_state.underrun_samples = 0;
            output_state.underrun_last_report = Instant::now();
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

#[cfg(test)]
mod tests {
    use super::{AudioOutputState, StreamingLinearResampler, write_audio_data};
    use std::collections::VecDeque;
    use std::f32::consts::PI;
    use std::sync::{Arc, Mutex};
    use std::time::Instant;

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
            underrun_last_report: Instant::now(),
        }));
        let mut output = [0.0f32; 4];

        write_audio_data(&mut output, 1, &output_state);

        assert_eq!(output[0], 0.25);
        assert!(output[1] < 0.25 && output[1] > 0.0);
        assert!(output[2] < output[1]);
        assert!(output[3] < output[2]);
    }
}
