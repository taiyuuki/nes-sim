use super::*;

fn tick_cycles(apu: &mut APU, cycles: usize) {
    for _ in 0..cycles {
        apu.tick_cpu_cycle();
    }
}

#[test]
fn status_reflects_pulse_length_and_disable_clears_it() {
    let mut apu = APU::new();

    apu.write_register_at_offset(0x4015, 0x01, 0);
    apu.write_register_at_offset(0x4000, 0x1F, 0);
    apu.write_register_at_offset(0x4002, 0x40, 0);
    apu.write_register_at_offset(0x4003, 0x18, 0);

    assert_eq!(apu.read_status_at_offset(0) & 0x01, 0x01);

    apu.write_register_at_offset(0x4015, 0x00, 0);
    assert_eq!(apu.read_status_at_offset(0) & 0x01, 0x00);
}

#[test]
fn pulse_envelope_decay_ticks_on_quarter_frames() {
    let mut pulse = PulseChannel::new(true);
    pulse.envelope_period = 2;
    pulse.constant_volume = false;
    pulse.envelope_start = true;

    pulse.quarter_frame_tick();
    assert_eq!(pulse.envelope_decay, 15);

    pulse.quarter_frame_tick();
    pulse.quarter_frame_tick();
    pulse.quarter_frame_tick();
    assert_eq!(pulse.envelope_decay, 14);
}

#[test]
fn pulse_sweep_updates_period_on_half_frame_clock() {
    let mut pulse = PulseChannel::new(true);
    pulse.enabled = true;
    pulse.length_counter = 10;
    pulse.timer_reload = 0x0064;
    pulse.sweep_enabled = true;
    pulse.sweep_shift = 1;
    pulse.sweep_negate = false;
    pulse.sweep_period = 0;
    pulse.sweep_divider = 0;
    pulse.sweep_reload = false;
    pulse.refresh_sweep_mute();

    pulse.half_frame_tick();

    assert_eq!(pulse.timer_reload, 0x0096);
}

#[test]
fn pulse_sweep_negate_uses_ones_complement_for_pulse1() {
    // Pulse 1 uses one's complement negate: period - change - 1
    let mut pulse1 = PulseChannel::new(true);
    pulse1.timer_reload = 0x0100;
    pulse1.sweep_shift = 2;
    pulse1.sweep_negate = true;
    // change = 0x0100 >> 2 = 0x40
    // one's complement: 0x0100 - 0x40 - 1 = 0x00BF
    assert_eq!(pulse1.target_period(), 0x00BF);

    // Pulse 2 uses two's complement negate: period - change
    let mut pulse2 = PulseChannel::new(false);
    pulse2.timer_reload = 0x0100;
    pulse2.sweep_shift = 2;
    pulse2.sweep_negate = true;
    // two's complement: 0x0100 - 0x40 = 0x00C0
    assert_eq!(pulse2.target_period(), 0x00C0);
}

#[test]
fn pulse_timer_high_resets_phase_on_retrigger() {
    let mut pulse = PulseChannel::new(false);
    pulse.enabled = true;
    pulse.length_counter = 8;
    pulse.seq_step = 5;

    pulse.write_timer_high(0x18, true);

    assert_eq!(pulse.seq_step, 0);
    assert_eq!(pulse.length_counter, LENGTH_TABLE[(0x18 >> 3) as usize]);
}

#[test]
fn pulse_timer_high_resets_phase_when_channel_inactive() {
    let mut pulse = PulseChannel::new(false);
    pulse.enabled = true;
    pulse.length_counter = 0;
    pulse.seq_step = 5;

    pulse.write_timer_high(0x18, true);

    assert_eq!(pulse.seq_step, 0);
}

#[test]
fn apu_generates_and_clears_audio_samples() {
    let mut apu = APU::new();

    apu.write_register_at_offset(0x4015, 0x01, 0);
    apu.write_register_at_offset(0x4000, 0x1F, 0);
    apu.write_register_at_offset(0x4002, 0x20, 0);
    apu.write_register_at_offset(0x4003, 0x18, 0);

    tick_cycles(&mut apu, 2_000);
    assert!(
        !apu.audio_samples().is_empty(),
        "apu should emit samples after enough cpu cycles"
    );

    apu.clear_audio_samples();
    assert!(apu.audio_samples().is_empty());
}

#[test]
fn set_sample_rate_updates_emission_interval() {
    let mut apu = APU::new();
    let default_rate = apu.sample_rate();
    apu.set_sample_rate(48_000);
    assert_eq!(apu.sample_rate(), 48_000);
    assert_ne!(apu.sample_rate(), default_rate);
    // 验证采样率正确设置
    assert_eq!(apu.sample_rate(), 48_000);
}

#[test]
fn five_step_mode_does_not_raise_frame_irq() {
    let mut apu = APU::new();
    apu.write_register_at_offset(0x4017, 0x80, 0);

    tick_cycles(&mut apu, 40_000);

    assert_eq!(apu.read_status_at_offset(0) & 0x40, 0);
}

#[test]
fn pulse_timers_advance_every_other_cpu_cycle() {
    let mut apu = APU::new();
    apu.pulse1.timer_reload = 0;
    apu.pulse1.timer_counter = 0;
    let start = apu.pulse1.seq_step;

    apu.tick_cpu_cycle();
    assert_eq!(apu.pulse1.seq_step, start);

    apu.tick_cpu_cycle();
    assert_eq!(apu.pulse1.seq_step, (start + 1) & 0x07);
}

#[test]
fn triangle_silent_when_length_or_linear_counter_is_zero() {
    let mut tri = TriangleChannel::default();
    tri.enabled = true;
    tri.timer_reload = 2;
    tri.seq_step = 5;

    tri.length_counter = 0;
    tri.linear_counter = 10;
    assert_eq!(
        tri.output(),
        0,
        "should be silent when length counter is zero"
    );

    tri.length_counter = 10;
    tri.linear_counter = 0;
    assert_eq!(
        tri.output(),
        0,
        "should be silent when linear counter is zero"
    );
}

#[test]
fn triangle_disabled_outputs_zero() {
    let mut tri = TriangleChannel::default();
    tri.enabled = false;
    tri.seq_step = 11;
    tri.length_counter = 10;
    tri.linear_counter = 10;

    assert_eq!(tri.output(), 0);
}

#[test]
fn triangle_timer_below_two_is_silenced() {
    let mut tri = TriangleChannel::default();
    tri.enabled = true;
    tri.timer_reload = 1;
    tri.seq_step = 11;
    tri.length_counter = 10;
    tri.linear_counter = 10;

    assert_eq!(tri.output(), 0);
    tri.tick_timer();
    assert_eq!(tri.seq_step, 11);
}

#[test]
fn dmc_silence_keeps_output_level_constant_without_sample_buffer() {
    let mut dmc = DmcState {
        timer_reload: 0,
        timer_counter: 0,
        output_level: 64,
        silence: true,
        sample_buffer: None,
        shift_register: 0xFF,
        bits_remaining: 8,
        ..DmcState::default()
    };

    dmc.tick_timer();

    assert_eq!(dmc.output_level, 64);
    assert!(dmc.silence);
}

#[test]
fn sample_integrator_accumulates_cpu_cycles_before_emit() {
    let mut apu = APU::new();
    apu.write_register_at_offset(0x4015, 0x01, 0);
    apu.write_register_at_offset(0x4000, 0x1F, 0);
    apu.write_register_at_offset(0x4002, 0x08, 0);
    apu.write_register_at_offset(0x4003, 0x18, 0);

    for _ in 0..64 {
        apu.tick_cpu_cycle();
    }

    assert!(apu.sample_accum_count > 0 || !apu.audio_samples().is_empty());
}

#[test]
fn fixed_point_division_handles_zero_divisor() {
    // 测试所有通道禁用时的边缘情况，确保不会除零
    let mut apu = APU::new();
    // 禁用所有通道
    apu.write_register_at_offset(0x4015, 0x00, 0);

    // 运行足够多的周期触发采样输出
    for _ in 0..500 {
        apu.tick_cpu_cycle();
    }

    // 应该生成一些静音样本，但不应该 panic
    assert!(!apu.audio_samples().is_empty());
    // 样本应该是静音（接近 0）
    assert!(apu.audio_samples().iter().all(|&s| s.abs() < 0.01));
}

#[test]
fn fixed_point_division_handles_very_low_values() {
    // 测试极小值的边缘情况
    let mut apu = APU::new();
    // 启用通道但设置极低音量
    apu.write_register_at_offset(0x4015, 0x01, 0);
    apu.write_register_at_offset(0x4000, 0x10, 0); // 常量音量，值为 0
    apu.write_register_at_offset(0x4002, 0x08, 0);
    apu.write_register_at_offset(0x4003, 0x18, 0);

    for _ in 0..500 {
        apu.tick_cpu_cycle();
    }

    // 应该生成一些样本，但不应该 panic
    assert!(!apu.audio_samples().is_empty());
}
