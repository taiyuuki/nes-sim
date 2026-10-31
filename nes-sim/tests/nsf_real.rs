// 真实NSF样本能量验证（本地开发用，无目录时跳过）
use nes_sim::NES;

#[test]
fn nsf_real_audible() {
    let Ok(dir) = std::env::var("NES_TEST_NSF_DIR") else {
        return;
    };
    let mut entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nsf")))
        .collect();
    entries.sort();
    let mut failures = Vec::new();
    for path in entries {
        let rom = std::fs::read(&path).expect("read");
        let mut nes = NES::new();
        if nes.load_rom(&rom, None).is_err() {
            continue;
        }
        let info = nes.nsf_info().unwrap().clone();
        // 播放8秒（480帧），统计峰值
        let mut peak = 0.0f32;
        for _ in 0..480 {
            nes.run_frame();
            for s in nes.apu_audio_samples() {
                peak = peak.max(s.abs());
            }
            nes.clear_apu_audio_samples();
        }
        let audible = peak > 0.01;
        println!(
            "{:<28} {:>18}  曲目{:>3}/{:<3} chips={:<2?} peak={:.4} {}",
            path.file_name().unwrap().to_string_lossy(),
            info.name,
            info.current_track,
            info.track_count,
            info.chip_names(),
            peak,
            if audible { "OK" } else { "!! SILENT" }
        );
        if !audible {
            failures.push(path.clone());
        }
    }
    assert!(failures.is_empty(), "silent fixtures: {failures:?}");
}
