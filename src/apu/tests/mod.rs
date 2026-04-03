use super::{APU, DmcDmaKind, DmcDmaRequest, PulseChannel};
use std::f32::consts::PI;

fn estimate_positive_zero_crossing_frequency(samples: &[f32], sample_rate: f32) -> f32 {
    let mut crossings = 0usize;
    for window in samples.windows(2) {
        if window[0] <= 0.0 && window[1] > 0.0 {
            crossings += 1;
        }
    }

    crossings as f32 * sample_rate / samples.len() as f32
}

fn pulse_cpu_cycles_per_full_wave(timer_period: u16) -> u64 {
    let mut pulse = PulseChannel::new(true);
    pulse.timer_period = timer_period;
    pulse.timer_value = timer_period;

    let start_step = pulse.sequence_step;
    let mut advances = 0u8;
    for cpu_cycle in 1..200_000u64 {
        if cpu_cycle & 1 == 0 {
            let before = pulse.sequence_step;
            pulse.clock_timer();
            if pulse.sequence_step != before {
                advances = advances.wrapping_add(1);
                if advances >= 8 && pulse.sequence_step == start_step {
                    return cpu_cycle;
                }
            }
        }
    }

    panic!("pulse wave did not complete within search window");
}

fn triangle_cpu_cycles_per_full_wave(timer_period: u16) -> u64 {
    let mut triangle = super::TriangleChannel::new();
    triangle.timer_period = timer_period;
    triangle.timer_value = timer_period;
    triangle.enabled = true;
    triangle.length_counter = 1;
    triangle.linear_counter = 1;

    let start_step = triangle.sequence_step;
    let mut advances = 0u8;
    for cpu_cycle in 1..200_000u64 {
        let before = triangle.sequence_step;
        triangle.clock_timer();
        if triangle.sequence_step != before {
            advances = advances.wrapping_add(1);
            if advances >= 32 && triangle.sequence_step == start_step {
                return cpu_cycle;
            }
        }
    }

    panic!("triangle wave did not complete within search window");
}

fn apu_with_pending_frame_irq() -> APU {
    let mut apu = APU::new();
    apu.frame_counter.irq_enabled = true;
    apu.frame_counter.irq_flag = true;
    apu
}

#[test]
fn read_4015_on_put_cycle_clears_before_following_get_cycle() {
    let mut apu = apu_with_pending_frame_irq();

    assert_eq!(apu.read_status_at_offset(5) & 0x40, 0x40);
    assert_eq!(apu.read_status_at_offset(6) & 0x40, 0x00);
}

#[test]
fn read_4015_on_get_cycle_stays_set_through_following_put_cycle() {
    let mut apu = apu_with_pending_frame_irq();

    assert_eq!(apu.read_status_at_offset(6) & 0x40, 0x40);
    assert_eq!(apu.read_status_at_offset(7) & 0x40, 0x40);
    assert_eq!(apu.read_status_at_offset(8) & 0x40, 0x00);
}

#[test]
fn write_4017_on_even_cycle_resets_frame_counter_after_three_cycles() {
    let mut apu = APU::new();

    apu.write_frame_counter_at_offset(0x00, 6);

    assert_eq!(apu.frame_counter.reset_delay, Some(3));
}

#[test]
fn write_4017_on_odd_cycle_resets_frame_counter_after_four_cycles() {
    let mut apu = APU::new();

    apu.write_frame_counter_at_offset(0x00, 5);

    assert_eq!(apu.frame_counter.reset_delay, Some(4));
}

#[test]
fn frame_irq_reassertion_cancels_pending_clear() {
    let mut apu = APU::new();
    apu.frame_counter.irq_enabled = true;
    apu.frame_counter.irq_flag = true;
    apu.frame_counter.irq_assert_window = 1;

    assert_eq!(apu.read_status_at_offset(6) & 0x40, 0x40);
    assert!(apu.frame_counter.irq_clear_after_cycle.is_some());

    apu.tick_cpu_cycle();

    assert_eq!(apu.frame_counter.irq_flag, true);
    assert_eq!(apu.frame_counter.irq_clear_after_cycle, None);
}

#[test]
fn frame_irq_line_goes_low_four_cycles_after_flag_first_sets() {
    let mut apu = APU::new();
    apu.frame_counter.irq_enabled = true;
    apu.frame_counter.cycle = 29_827;

    apu.tick_cpu_cycle();
    assert!(apu.frame_counter.irq_flag);
    assert!(!apu.irq_line());

    apu.tick_cpu_cycle();
    assert!(!apu.irq_line());

    apu.tick_cpu_cycle();
    assert!(!apu.irq_line());

    apu.tick_cpu_cycle();
    assert!(!apu.irq_line());

    apu.tick_cpu_cycle();
    assert!(apu.irq_line());
}

#[test]
fn pulse_sweep_reload_sets_divider_without_immediate_period_change() {
    let mut pulse = PulseChannel::new(true);
    pulse.timer_period = 0x0400;
    pulse.sweep_divider = 2;
    pulse.write_sweep(0x91);

    pulse.clock_sweep();

    assert_eq!(pulse.timer_period, 0x0400);
    assert_eq!(pulse.sweep_divider, 1);
    assert!(!pulse.sweep_reload);
}

#[test]
fn pulse_one_and_two_negate_sweep_use_different_subtraction() {
    let mut pulse1 = PulseChannel::new(true);
    let mut pulse2 = PulseChannel::new(false);

    pulse1.timer_period = 0x0100;
    pulse2.timer_period = 0x0100;
    pulse1.sweep_enabled = true;
    pulse2.sweep_enabled = true;
    pulse1.sweep_negate = true;
    pulse2.sweep_negate = true;
    pulse1.sweep_shift = 1;
    pulse2.sweep_shift = 1;
    pulse1.sweep_divider = 0;
    pulse2.sweep_divider = 0;

    pulse1.clock_sweep();
    pulse2.clock_sweep();

    assert_eq!(pulse1.timer_period, 0x007F);
    assert_eq!(pulse2.timer_period, 0x0080);
}

#[test]
fn pulse_sweep_mutes_output_when_target_period_overflows() {
    let mut pulse = PulseChannel::new(true);
    pulse.enabled = true;
    pulse.length_counter = 1;
    pulse.constant_volume = true;
    pulse.volume = 15;
    pulse.duty = 2;
    pulse.sequence_step = 1;
    pulse.timer_period = 0x07FF;
    pulse.sweep_shift = 1;

    assert_eq!(pulse.output(), 0.0);
}

#[test]
fn pulse_channel_generates_non_zero_audio_samples() {
    let mut apu = APU::new();
    apu.write_register_at_offset(0x4015, 0x01, 0);
    apu.write_register_at_offset(0x4000, 0x1F, 0);
    apu.write_register_at_offset(0x4002, 0x20, 0);
    apu.write_register_at_offset(0x4003, 0x08, 0);

    for _ in 0..10_000 {
        apu.tick_cpu_cycle();
    }

    assert!(!apu.audio_samples().is_empty());
    assert!(
        apu.audio_samples()
            .iter()
            .any(|sample| sample.abs() > 0.0001)
    );
}

#[test]
fn triangle_channel_generates_non_zero_audio_samples() {
    let mut apu = APU::new();
    apu.write_register_at_offset(0x4015, 0x04, 0);
    apu.write_register_at_offset(0x4008, 0x8F, 0);
    apu.write_register_at_offset(0x400A, 0x10, 0);
    apu.write_register_at_offset(0x400B, 0x08, 0);

    for _ in 0..8_000 {
        apu.tick_cpu_cycle();
    }

    assert!(apu.channels.triangle.linear_counter > 0);
    assert!(apu.channels.triangle.length_counter > 0);
    assert!(apu.channels.triangle.output() > 0.0);

    apu.clear_audio_samples();
    for _ in 0..512 {
        apu.tick_cpu_cycle();
    }

    assert!(!apu.audio_samples().is_empty());
    assert!(
        apu.audio_samples()
            .iter()
            .any(|sample| sample.abs() > 0.0001)
    );
}

#[test]
fn noise_channel_generates_non_zero_audio_samples() {
    let mut apu = APU::new();
    apu.write_register_at_offset(0x4015, 0x08, 0);
    apu.write_register_at_offset(0x400C, 0x1F, 0);
    apu.write_register_at_offset(0x400E, 0x00, 0);
    apu.write_register_at_offset(0x400F, 0x08, 0);

    for _ in 0..10_000 {
        apu.tick_cpu_cycle();
    }

    assert!(!apu.audio_samples().is_empty());
    assert!(
        apu.audio_samples()
            .iter()
            .any(|sample| sample.abs() > 0.0001)
    );
}

#[test]
fn dmc_enable_sets_status_bit_and_exposes_dma_request() {
    let mut apu = APU::new();
    apu.write_register_at_offset(0x4012, 0x34, 0);
    apu.write_register_at_offset(0x4013, 0x01, 0);
    apu.write_register_at_offset(0x4015, 0x10, 0);

    assert_eq!(apu.read_status_at_offset(0) & 0x10, 0x10);
    assert_eq!(
        apu.take_dmc_dma_request(),
        Some(DmcDmaRequest {
            addr: 0xCD00,
            kind: DmcDmaKind::Load,
        })
    );
}

#[test]
fn dmc_direct_load_contributes_to_audio_mix() {
    let mut apu = APU::new();
    apu.write_register_at_offset(0x4011, 0x40, 0);

    for _ in 0..256 {
        apu.tick_cpu_cycle();
    }

    assert!(!apu.audio_samples().is_empty());
    assert!(
        apu.audio_samples()
            .iter()
            .any(|sample| sample.abs() > 0.0001)
    );
}

#[test]
fn constant_dmc_level_does_not_leave_persistent_dc_offset() {
    let mut apu = APU::new();
    apu.write_register_at_offset(0x4011, 0x40, 0);

    for _ in 0..200_000 {
        apu.tick_cpu_cycle();
    }

    apu.clear_audio_samples();

    for _ in 0..50_000 {
        apu.tick_cpu_cycle();
    }

    assert!(!apu.audio_samples().is_empty());
    assert!(apu.audio_samples().iter().all(|sample| sample.abs() < 0.01));
}

#[test]
fn resampler_rejects_cpu_rate_nyquist_toggle_noise() {
    let mut resampler = super::AudioResampler::new();
    let mut output = Vec::new();

    for index in 0..200_000 {
        let sample = if index & 1 == 0 { 1.0 } else { -1.0 };
        resampler.push_source_sample(sample, &mut output);
    }

    assert!(!output.is_empty());
    let average_abs = output.iter().map(|sample| sample.abs()).sum::<f32>() / output.len() as f32;
    assert!(average_abs < 0.05);
}

#[test]
fn resampler_preserves_input_tone_frequency() {
    let mut resampler = super::AudioResampler::new();
    let mut output = Vec::new();
    let tone_hz = 440.0f32;
    let input_rate = 1_789_773.0f32;
    let phase_step = 2.0 * PI * tone_hz / input_rate;
    let mut phase = 0.0f32;

    for _ in 0..300_000 {
        resampler.push_source_sample(phase.sin(), &mut output);
        phase += phase_step;
    }

    let expected_len = 300_000usize * 44_100 / 1_789_773usize;
    assert!(
        output.len().abs_diff(expected_len) <= 2,
        "expected about {expected_len} output samples, got {}",
        output.len()
    );

    let measured_hz = estimate_positive_zero_crossing_frequency(&output[2048..], 44_100.0);
    assert!(
        (measured_hz - tone_hz).abs() < 10.0,
        "expected about {tone_hz:.2} Hz, measured {measured_hz:.2} Hz"
    );
}

#[test]
fn pulse_channel_output_frequency_matches_ntsc_timer_formula() {
    let mut apu = APU::new();
    let timer_period = 253u16;
    let expected_hz = 1_789_773.0 / (16.0 * (timer_period + 1) as f32);

    apu.write_register_at_offset(0x4015, 0x01, 0);
    apu.write_register_at_offset(0x4000, 0x9F, 0);
    apu.write_register_at_offset(0x4001, 0x08, 0);
    apu.write_register_at_offset(0x4002, timer_period as u8, 0);
    apu.write_register_at_offset(0x4003, ((timer_period >> 8) as u8) | 0xF8, 0);

    for _ in 0..40_000 {
        apu.tick_cpu_cycle();
    }

    apu.clear_audio_samples();

    for _ in 0..200_000 {
        apu.tick_cpu_cycle();
    }

    let measured_hz =
        estimate_positive_zero_crossing_frequency(apu.audio_samples(), apu.sample_rate() as f32);

    assert!(
        (measured_hz - expected_hz).abs() < 10.0,
        "expected about {expected_hz:.2} Hz, measured {measured_hz:.2} Hz"
    );
}

#[test]
fn triangle_channel_output_frequency_matches_ntsc_timer_formula() {
    let mut apu = APU::new();
    let timer_period = 126u16;
    let expected_hz = 1_789_773.0 / (32.0 * (timer_period + 1) as f32);

    apu.write_register_at_offset(0x4015, 0x04, 0);
    apu.write_register_at_offset(0x4008, 0x8F, 0);
    apu.write_register_at_offset(0x400A, timer_period as u8, 0);
    apu.write_register_at_offset(0x400B, ((timer_period >> 8) as u8) | 0xF8, 0);

    for _ in 0..40_000 {
        apu.tick_cpu_cycle();
    }

    apu.clear_audio_samples();

    for _ in 0..200_000 {
        apu.tick_cpu_cycle();
    }

    let measured_hz =
        estimate_positive_zero_crossing_frequency(apu.audio_samples(), apu.sample_rate() as f32);

    assert!(
        (measured_hz - expected_hz).abs() < 3.0,
        "expected about {expected_hz:.2} Hz, measured {measured_hz:.2} Hz"
    );
}

#[test]
fn dmc_control_write_does_not_reset_active_timer_phase() {
    let mut dmc = super::DmcChannel::new();
    dmc.enabled = true;
    dmc.bytes_remaining = 1;
    dmc.sample_buffer = Some(0x00);
    dmc.timer_value = 7;

    dmc.write_control(0x4F);

    assert_eq!(dmc.timer_value, 7);
}

#[test]
fn dmc_fastest_rate_clocks_output_after_54_cpu_cycles() {
    let mut dmc = super::DmcChannel::new();
    dmc.write_control(0x0F);
    dmc.bits_remaining = 1;

    for _ in 0..53 {
        dmc.clock_timer();
    }
    assert_eq!(dmc.bits_remaining, 1);

    dmc.clock_timer();
    assert_eq!(dmc.bits_remaining, 8);
}

#[test]
fn pulse_timer_sequence_matches_ntsc_frequency_formula_in_cpu_cycles() {
    let timer_period = 253u16;
    let expected_cycles = 16 * u64::from(timer_period + 1);

    assert_eq!(
        pulse_cpu_cycles_per_full_wave(timer_period),
        expected_cycles
    );
}

#[test]
fn triangle_timer_sequence_matches_ntsc_frequency_formula_in_cpu_cycles() {
    let timer_period = 126u16;
    let expected_cycles = 32 * u64::from(timer_period + 1);

    assert_eq!(
        triangle_cpu_cycles_per_full_wave(timer_period),
        expected_cycles
    );
}
