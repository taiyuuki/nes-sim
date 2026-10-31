//! NSF播放集成测试：手写最小NSF（2A03/VRC6）+ 可选真实样本。

use super::NES;
use crate::nsf::{NSF_HEADER_LEN, NsfHeader, parse_nsf};

/// 组装一个最小NSF文件。
struct NsfBuilder {
    header: [u8; NSF_HEADER_LEN],
    code: Vec<u8>,
}

impl NsfBuilder {
    fn new(load_addr: u16, init_addr: u16, play_addr: u16) -> Self {
        let mut header = [0u8; NSF_HEADER_LEN];
        header[0..5].copy_from_slice(b"NESM\x1a");
        header[5] = 1; // version
        header[6] = 1; // total songs
        header[7] = 1; // start song
        header[8..10].copy_from_slice(&load_addr.to_le_bytes());
        header[10..12].copy_from_slice(&init_addr.to_le_bytes());
        header[12..14].copy_from_slice(&play_addr.to_le_bytes());
        header[0x6E..0x70].copy_from_slice(&16_666u16.to_le_bytes()); // NTSC speed
        // banks全0：不用bank切换；pal=0；chips=0（纯2A03）
        Self {
            header,
            code: Vec::new(),
        }
    }

    fn songs(mut self, count: u8, start: u8) -> Self {
        self.header[6] = count;
        self.header[7] = start;
        self
    }

    fn chips(mut self, flags: u8) -> Self {
        self.header[0x7B] = flags;
        self
    }

    fn text(mut self, name: &str, artist: &str) -> Self {
        for (i, b) in name.bytes().enumerate() {
            self.header[0x0E + i] = b;
        }
        for (i, b) in artist.bytes().enumerate() {
            self.header[0x2E + i] = b;
        }
        self
    }

    fn code(mut self, code: &[u8]) -> Self {
        self.code = code.to_vec();
        self
    }

    fn build(self) -> Vec<u8> {
        let mut rom = self.header.to_vec();
        rom.extend_from_slice(&self.code);
        rom
    }
}

/// 2A03脉冲1播放器：INIT保存曲目号并开$4015，
/// PLAY按曲目设置脉冲周期（曲目号×$20偏移）。
/// track0周期=$100 → 1789773/(16×257) ≈ 435.4Hz；
/// track1周期=$120 → 1789773/(16×289) ≈ 387.2Hz。
fn pulse_nsf(songs: u8, start: u8) -> Vec<u8> {
    // $8000 INIT:
    //   85 00      STA $00      ; 保存曲目号（A=track由调用约定传入）
    //   A9 0F      LDA #$0F
    //   8D 15 40   STA $4015
    //   60         RTS
    // $8008 PLAY:
    //   A6 00      LDX $00
    //   8A         TXA
    //   0A×5       ASL A ×5     ; A = track<<5
    //   8D 02 40   STA $4002    ; 周期低字节
    //   A9 BF      LDA #$BF     ; duty=10 常量音量15
    //   8D 00 40   STA $4000
    //   A9 01      LDA #$01     ; 周期高位=1, length idx=0
    //   8D 03 40   STA $4003
    //   60         RTS
    let code = [
        0x85, 0x00, 0xA9, 0x0F, 0x8D, 0x15, 0x40, 0x60, // INIT @ $8000
        0xA6, 0x00, 0x8A, 0x0A, 0x0A, 0x0A, 0x0A, 0x0A, 0x8D, 0x02, 0x40, 0xA9, 0xBF, 0x8D, 0x00,
        0x40, 0xA9, 0x01, 0x8D, 0x03, 0x40, 0x60, // PLAY @ $8008
    ];
    NsfBuilder::new(0x8000, 0x8000, 0x8008)
        .songs(songs, start)
        .text("Pulse Test", "nes-sim")
        .code(&code)
        .build()
}

/// 收集播放若干帧后的音频能量与主频估计（符号变化法）。
fn play_and_analyze(nes: &mut NES, frames: usize) -> (f32, f32) {
    let sample_rate = nes.apu_sample_rate();
    let mut samples = Vec::new();
    for _ in 0..frames {
        nes.run_frame();
        samples.extend_from_slice(nes.apu_audio_samples());
        nes.clear_apu_audio_samples();
    }
    let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    // 中值阈值沿计数估计主频（APU输出为单极性方波，过零法不适用）
    let midpoint = peak / 2.0;
    let mut rising_edges = 0usize;
    let mut prev_high = samples.first().map(|s| *s > midpoint).unwrap_or(false);
    for s in &samples {
        let high = *s > midpoint;
        if high && !prev_high {
            rising_edges += 1;
        }
        prev_high = high;
    }
    let duration = samples.len() as f32 / sample_rate as f32;
    let freq = rising_edges as f32 / duration;
    (peak, freq)
}

#[test]
fn nsf_header_parse_and_info() {
    let rom = pulse_nsf(1, 1);
    assert!(crate::nsf::is_nsf_image(&rom));
    let header: NsfHeader = parse_nsf(&rom).expect("parse");
    assert_eq!(header.total_songs, 1);
    assert_eq!(header.load_addr, 0x8000);
    assert!((header.play_speed_us(crate::cartridge::TVSystem::NTSC) - 16_666) == 0);

    let mut nes = NES::new();
    nes.load_rom(&rom, None).expect("load nsf");
    let info = nes.nsf_info().expect("nsf info");
    assert_eq!(info.name, "Pulse Test");
    assert_eq!(info.artist, "nes-sim");
    assert_eq!(info.track_count, 1);
    assert_eq!(info.current_track, 1);
}

#[test]
fn nsf_plays_pulse_tone() {
    let rom = pulse_nsf(1, 1);
    let mut nes = NES::new();
    nes.set_apu_sample_rate(48_000);
    nes.load_rom(&rom, None).expect("load nsf");
    let (peak, freq) = play_and_analyze(&mut nes, 30);
    assert!(peak > 0.05, "audio silent: peak={peak}");
    // track0周期$100 → 1789773/(16×257) ≈ 435.4Hz
    assert!(
        (freq - 435.4).abs() < 40.0,
        "unexpected frequency: {freq} Hz"
    );
}

#[test]
fn nsf_select_track_changes_tone() {
    let rom = pulse_nsf(2, 1);
    let mut nes = NES::new();
    nes.set_apu_sample_rate(48_000);
    nes.load_rom(&rom, None).expect("load nsf");
    let (peak1, freq1) = play_and_analyze(&mut nes, 20);
    assert!(peak1 > 0.05);
    assert!((freq1 - 435.4).abs() < 40.0, "track1 freq: {freq1}");

    assert!(nes.nsf_select_track(2));
    assert_eq!(nes.nsf_track(), Some(2));
    let (_peak2, freq2) = play_and_analyze(&mut nes, 20);
    // track1周期$120 → 1789773/(16×289) ≈ 387.2Hz
    assert!((freq2 - 387.2).abs() < 40.0, "track2 freq: {freq2}");
    // 越界钳制
    assert!(nes.nsf_select_track(99));
    assert_eq!(nes.nsf_track(), Some(2));
}

#[test]
fn nsf_vrc6_expansion_plays() {
    // VRC6 pulse1：$9000=音量63，$9001=周期低，$9002=bit7使能+周期高
    // INIT:  A9 FF 8D 00 90  8D 01 90  8D 02 90  60
    // PLAY:  同样重写三个寄存器后 RTS
    let code = [
        0xA9, 0x3F, 0x8D, 0x00, 0x90, // LDA #$3F STA $9000
        0xA9, 0x00, 0x8D, 0x01, 0x90, // LDA #$00 STA $9001
        0xA9, 0x83, 0x8D, 0x02, 0x90, // LDA #$83 STA $9002
        0x60, // RTS (INIT结束，PLAY从$800F开始重复同样序列)
        0xA9, 0x3F, 0x8D, 0x00, 0x90, 0xA9, 0x00, 0x8D, 0x01, 0x90, 0xA9, 0x83, 0x8D, 0x02, 0x90,
        0x60,
    ];
    let rom = NsfBuilder::new(0x8000, 0x8000, 0x800F)
        .chips(0x01) // VRC6
        .code(&code)
        .build();
    let mut nes = NES::new();
    nes.load_rom(&rom, None).expect("load nsf");
    let (peak, freq) = play_and_analyze(&mut nes, 30);
    assert!(peak > 0.05, "VRC6 audio silent: peak={peak}");
    // VRC6周期 $300=768 → 1789773/(16*769) ≈ 145.6Hz
    assert!(
        (freq - 145.6).abs() < 15.0,
        "unexpected VRC6 frequency: {freq} Hz"
    );
}

#[test]
fn nsf_savestate_roundtrip() {
    let rom = pulse_nsf(1, 1);
    let mut nes = NES::new();
    nes.load_rom(&rom, None).expect("load nsf");
    for _ in 0..5 {
        nes.run_frame();
        nes.clear_apu_audio_samples();
    }
    let state = nes.save_state().expect("save");
    // 继续播放几帧后恢复
    for _ in 0..5 {
        nes.run_frame();
    }
    nes.clear_apu_audio_samples();
    nes.load_state(&state).expect("load");
    let info = nes.nsf_info().expect("still nsf");
    assert_eq!(info.current_track, 1);
    let (peak, _) = play_and_analyze(&mut nes, 10);
    assert!(peak > 0.05, "audio silent after state load");
}

#[test]
fn nsf_reset_restarts_track() {
    let rom = pulse_nsf(1, 1);
    let mut nes = NES::new();
    nes.load_rom(&rom, None).expect("load nsf");
    let (peak, _) = play_and_analyze(&mut nes, 10);
    assert!(peak > 0.05);
    nes.execute(crate::CoreCommand::Reset);
    let (peak2, _) = play_and_analyze(&mut nes, 10);
    assert!(peak2 > 0.05, "audio silent after reset");
}

/// 真实样本冒烟测试：设置 `NES_TEST_NSF_DIR` 指向NSF目录后启用。
/// 每个文件加载、播放10帧，断言元数据可读且不panic（不强制有声——
/// 某些曲目前几帧静音是正常的）。
#[test]
fn nsf_real_fixtures_smoke() {
    let Ok(dir) = std::env::var("NES_TEST_NSF_DIR") else {
        return;
    };
    let mut tested = 0;
    for entry in std::fs::read_dir(&dir).expect("read NSF dir") {
        let path = entry.expect("dir entry").path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !ext.eq_ignore_ascii_case("nsf") {
            continue;
        }
        let rom = std::fs::read(&path).expect("read nsf");
        if !crate::nsf::is_nsf_image(&rom) {
            continue;
        }
        let mut nes = NES::new();
        match nes.load_rom(&rom, None) {
            Ok(()) => {
                let info = nes.nsf_info().expect("info");
                assert!(info.track_count >= 1, "{}: track_count", path.display());
                // 第二曲存在则切换测试
                if info.track_count > 1 {
                    assert!(nes.nsf_select_track(2), "{}: select track", path.display());
                }
                for _ in 0..10 {
                    nes.run_frame();
                }
            }
            Err(e) => panic!("{}: load failed: {:?}", path.display(), e),
        }
        tested += 1;
    }
    eprintln!("NSF fixture smoke tested {tested} files");
}
