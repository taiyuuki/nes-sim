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
