use super::port_kind;
use super::*;
use crate::input::ControllerState;
use crate::{CoreCommand, CoreEvent, NES};

const SAMPLE_FM2: &str = "\
version 3\r\n\
emuVersion 9828\r\n\
rerecordCount 42\r\n\
palFlag 0\r\n\
romFilename Contra (U) [!].nes\r\n\
romChecksum 8A62A32AB1B08C4BE1DEAE40C2873B0E\r\n\
guid 452DE2C3-EF43-2FA9-77AC-0677FC51543B\r\n\
fourscore 0\r\n\
microphone 0\r\n\
port0 1\r\n\
port1 1\r\n\
port2 0\r\n\
comment author adelikat\r\n\
comment lorem ipsum\r\n\
subtitle 1000 Level Two\r\n\
unknownKey ignored\r\n\
|0|........|........||\r\n\
|0|RLDUTSBA|........||\r\n\
|1|.......A|......B.||\r\n\
";

#[test]
fn parses_header_keys() {
    let movie = parse_fm2(SAMPLE_FM2.as_bytes()).unwrap();
    assert_eq!(movie.version, 3);
    assert_eq!(movie.emu_version, 9828);
    assert_eq!(movie.rerecord_count, 42);
    assert!(!movie.pal_flag);
    assert_eq!(movie.rom_filename, "Contra (U) [!].nes");
    assert_eq!(movie.rom_checksum, "8A62A32AB1B08C4BE1DEAE40C2873B0E");
    assert_eq!(movie.guid, "452DE2C3-EF43-2FA9-77AC-0677FC51543B");
    assert!(!movie.fourscore);
    assert!(!movie.microphone);
    assert_eq!(
        movie.ports,
        [port_kind::GAMEPAD, port_kind::GAMEPAD, port_kind::NONE]
    );
    assert_eq!(movie.comments, ["author adelikat", "lorem ipsum"]);
    assert_eq!(movie.subtitles, ["1000 Level Two"]);
    assert!(!movie.starts_from_savestate);
    assert_eq!(movie.frames.len(), 3);
}

#[test]
fn input_line_bit_order_matches_controller_state() {
    let movie = parse_fm2(SAMPLE_FM2.as_bytes()).unwrap();
    // 全松开
    assert_eq!(
        movie.frames[0].controllers,
        [ControllerState::new(), ControllerState::new()]
    );
    // RLDUTSBA = bit7..bit0 全按下，恰好是 NES 手柄移位寄存器顺序
    assert_eq!(movie.frames[1].controllers[0].bits(), 0xFF);
    assert_eq!(movie.frames[1].controllers[1].bits(), 0x00);
    // ".......A" → A(bit0)；"......B." → B(bit1)
    assert_eq!(movie.frames[2].controllers[0].bits(), 0x01);
    assert_eq!(movie.frames[2].controllers[1].bits(), 0x02);
    // commands：第 3 帧 reset
    assert!(!movie.frames[0].reset());
    assert!(movie.frames[2].reset());
}

#[test]
fn space_and_arbitrary_chars_count_as_pressed_or_released() {
    let text = "version 3\n|0|R.d tS A|........||\n";
    let movie = parse_fm2(text.as_bytes()).unwrap();
    // ' ' 和 '.' 为松开，其余字符（含随意字母）为按下：
    // R(bit7) d(bit5) t(bit3) S(bit2) A(bit0)
    let bits = movie.frames[0].controllers[0].bits();
    assert_eq!(bits, 0b1010_1101);
}

#[test]
fn parses_fourscore_layout() {
    let text = "\
version 3\n\
fourscore 1\n\
|0|RLDUTSBA|......B.|.......A|........||\n\
";
    let movie = parse_fm2(text.as_bytes()).unwrap();
    assert!(movie.fourscore);
    assert_eq!(movie.frames.len(), 1);
    // 前两手柄回放
    assert_eq!(movie.frames[0].controllers[0].bits(), 0xFF);
    assert_eq!(movie.frames[0].controllers[1].bits(), 0x02);
}

#[test]
fn port_none_field_is_empty() {
    let text = "\
version 3\n\
port0 1\n\
port1 0\n\
|0|.......A||\n\
";
    let movie = parse_fm2(text.as_bytes()).unwrap();
    assert_eq!(movie.ports[1], port_kind::NONE);
    assert_eq!(movie.frames[0].controllers[0].bits(), 0x01);
    assert_eq!(movie.frames[0].controllers[1].bits(), 0x00);
}

#[test]
fn rejects_zapper_and_bad_lines() {
    let zapper = "version 3\nport0 2\n|0|.........|........||\n";
    assert_eq!(
        parse_fm2(zapper.as_bytes()).unwrap_err(),
        MovieError::UnsupportedPort(port_kind::ZAPPER)
    );

    // 与 FCEUX 一致：文件必须以 "version 3" 开头，其他版本号直接拒绝
    assert_eq!(parse_fm2(b"version 2\n").unwrap_err(), MovieError::NotAnFm2);
    assert_eq!(parse_fm2(b"garbage").unwrap_err(), MovieError::NotAnFm2);
    // 头部中逄出现的 version 键非 3 → 版本错误
    let dup = "version 3\nversion 4\n";
    assert_eq!(
        parse_fm2(dup.as_bytes()).unwrap_err(),
        MovieError::UnsupportedVersion(4)
    );

    let bad_line = "version 3\n|0|short|........||\n";
    assert_eq!(
        parse_fm2(bad_line.as_bytes()).unwrap_err(),
        MovieError::BadInputLine(2)
    );
}

#[test]
fn parses_binary_variant() {
    let mut bytes = b"\
version 3\n\
port0 1\n\
port1 1\n\
binary 1\n\
|"
    .to_vec();
    // 3 帧：命令 + port0 + port1
    bytes.extend_from_slice(&[0x00, 0x00, 0x00]); // 全松开
    bytes.extend_from_slice(&[0x01, 0xFF, 0x02]); // reset + 两手柄
    bytes.extend_from_slice(&[0x00, 0x81, 0x00]);

    let movie = parse_fm2(&bytes).unwrap();
    assert_eq!(movie.frames.len(), 3);
    assert_eq!(movie.frames[1].commands, 1);
    assert!(movie.frames[1].reset());
    assert_eq!(movie.frames[1].controllers[0].bits(), 0xFF);
    assert_eq!(movie.frames[1].controllers[1].bits(), 0x02);
    assert_eq!(movie.frames[2].controllers[0].bits(), 0x81);

    // 尾部多余字节 → 报错
    bytes.push(0);
    assert_eq!(
        parse_fm2(&bytes).unwrap_err(),
        MovieError::TruncatedBinaryLog
    );
}

#[test]
fn length_key_truncates_input_log() {
    // fm3 约定：length 截断输入日志（其后可能有工程数据）
    let mut text = SAMPLE_FM2.to_string();
    text.insert_str(text.find('|').unwrap(), "length 2\n");
    let movie = parse_fm2(text.as_bytes()).unwrap();
    assert_eq!(movie.frames.len(), 2);
}

#[test]
fn write_fm2_roundtrips() {
    let movie = parse_fm2(SAMPLE_FM2.as_bytes()).unwrap();
    let text = movie.to_fm2_string();
    let reparsed = parse_fm2(text.as_bytes()).unwrap();
    assert_eq!(reparsed, movie);

    // 输出行格式：双管道结尾、RLDUTSBA 助记
    assert!(text.contains("|0|RLDUTSBA|........||\n"));
    assert!(text.contains("|1|.......A|......B.||\n"));

    // fourscore 写出为 4 手柄字段
    let mut fs = movie.clone();
    fs.fourscore = true;
    let text = fs.to_fm2_string();
    let reparsed = parse_fm2(text.as_bytes()).unwrap();
    assert!(reparsed.fourscore);
    assert_eq!(reparsed.frames[1].controllers[0].bits(), 0xFF);
    assert!(text.contains("|0|RLDUTSBA|........|........|........||\n"));
}

// ---- 核心集成 -------------------------------------------------------------

/// 最小 NROM 测试卡带（与 cheat 测试同款）：程序立即写 $0700 的读取值到 $0050。
fn make_test_rom() -> Vec<u8> {
    let mut prg = vec![0u8; 0x4000];
    let program = [
        0xA9, 0x00, // LDA #$00
        0x8D, 0x00, 0x07, // STA $0700
        0xAD, 0x00, 0x07, // LDA $0700
        0x8D, 0x50, 0x00, // STA $0050
        0x4C, 0x0B, 0x80, // JMP $800B
    ];
    prg[..program.len()].copy_from_slice(&program);
    prg[0x3FFC] = 0x00;
    prg[0x3FFD] = 0x80;

    let mut rom = vec![0u8; 16];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 0;
    rom.extend_from_slice(&prg);
    rom.extend_from_slice(&[0u8; 0x2000]);
    rom
}

fn frame(bits0: u8, bits1: u8, commands: u32) -> MovieFrame {
    MovieFrame {
        commands,
        controllers: [
            ControllerState::from_bits(bits0),
            ControllerState::from_bits(bits1),
        ],
    }
}

#[test]
fn playback_applies_inputs_and_finishes() {
    let rom = make_test_rom();
    let mut nes = NES::new();
    nes.load_cartridge_ines(&rom).unwrap();
    nes.reset();

    let movie = Movie {
        version: 3,
        ports: [port_kind::GAMEPAD, port_kind::GAMEPAD, port_kind::NONE],
        frames: vec![
            frame(0x81, 0x00, 0),
            frame(0x00, 0x42, 0),
            frame(0x00, 0x00, 1),
        ],
        ..Movie::default()
    };
    let len = movie.frames.len();
    let response = nes.execute(CoreCommand::PlayMovie(movie));
    assert_eq!(response.event, CoreEvent::None);
    assert_eq!(nes.movie_frame(), Some(0));

    // 前端输入会被播放器覆盖
    nes.execute(CoreCommand::SetControllerState {
        port: 0,
        state: ControllerState::from_bits(0xFF),
    });
    let response = nes.execute(CoreCommand::RunFrame);
    assert_eq!(response.event, CoreEvent::FrameReady { frame_number: 1 });
    assert_eq!(nes.movie_frame(), Some(1));
    assert_eq!(nes.bus.controller_state(0).bits(), 0x81);
    assert_eq!(nes.bus.controller_state(1).bits(), 0x00);

    nes.execute(CoreCommand::RunFrame);
    assert_eq!(nes.bus.controller_state(1).bits(), 0x42);

    // 最后一帧（含 reset 命令）→ MovieFinished
    let response = nes.execute(CoreCommand::RunFrame);
    assert_eq!(response.event, CoreEvent::MovieFinished);
    assert_eq!(nes.movie_frame(), Some(len));

    // 播完后继续跑帧不套用录像输入，前端恢复控制
    nes.execute(CoreCommand::SetControllerState {
        port: 0,
        state: ControllerState::from_bits(0x10),
    });
    nes.execute(CoreCommand::RunFrame);
    assert_eq!(nes.bus.controller_state(0).bits(), 0x10);

    // StopMovie 清除
    nes.execute(CoreCommand::StopMovie);
    assert!(nes.movie().is_none());
}

#[test]
fn empty_movie_finishes_immediately() {
    let mut nes = NES::new();
    let response = nes.execute(CoreCommand::PlayMovie(Movie::default()));
    assert_eq!(response.event, CoreEvent::MovieFinished);
}

#[test]
fn recording_captures_inputs_and_reset() {
    let rom = make_test_rom();
    let mut nes = NES::new();
    nes.load_cartridge_ines(&rom).unwrap();
    nes.reset();

    nes.execute(CoreCommand::StartMovieRecording);
    assert!(nes.is_recording_movie());

    nes.execute(CoreCommand::SetControllerState {
        port: 0,
        state: ControllerState::from_bits(0x81),
    });
    nes.execute(CoreCommand::RunFrame);

    nes.execute(CoreCommand::Reset);
    nes.execute(CoreCommand::RunFrame);

    nes.execute(CoreCommand::SetControllerState {
        port: 1,
        state: ControllerState::from_bits(0x42),
    });
    nes.execute(CoreCommand::RunFrame);

    nes.execute(CoreCommand::StopMovieRecording);
    assert!(!nes.is_recording_movie());

    let movie = nes.take_recorded_movie().expect("recorded movie");
    assert!(nes.take_recorded_movie().is_none()); // 只能取一次
    assert_eq!(movie.frames.len(), 3);
    assert_eq!(movie.frames[0].controllers[0].bits(), 0x81);
    assert!(!movie.frames[0].reset());
    assert!(movie.frames[1].reset()); // Reset 命令记录在该帧
    assert_eq!(movie.frames[2].controllers[1].bits(), 0x42);

    // 录像可回放：to_fm2_string → parse_fm2 一致
    let text = movie.to_fm2_string();
    let reparsed = parse_fm2(text.as_bytes()).unwrap();
    assert_eq!(reparsed.frames, movie.frames);
    assert!(text.contains("port0 1"));
    assert!(text.contains("port2 0"));
}

#[test]
fn recorded_movie_replays_identically() {
    // 录制 → 回放，两次机器状态轨迹一致（哈希对比）
    let rom = make_test_rom();

    let mut recorder = NES::new();
    recorder.load_cartridge_ines(&rom).unwrap();
    recorder.reset();
    recorder.execute(CoreCommand::StartMovieRecording);
    let inputs = [0x81u8, 0x00, 0x42, 0xFF, 0x00];
    for &bits in &inputs {
        recorder.execute(CoreCommand::SetControllerState {
            port: 0,
            state: ControllerState::from_bits(bits),
        });
        recorder.execute(CoreCommand::RunFrame);
    }
    recorder.execute(CoreCommand::StopMovieRecording);
    let movie = recorder.take_recorded_movie().unwrap();

    let mut player = NES::new();
    player.load_cartridge_ines(&rom).unwrap();
    player.reset();
    player.execute(CoreCommand::PlayMovie(movie));
    for _ in 0..inputs.len() {
        player.execute(CoreCommand::RunFrame);
    }

    assert_eq!(recorder.bus.ram, player.bus.ram);
    assert_eq!(recorder.save_state().unwrap(), player.save_state().unwrap());
}

#[test]
fn movie_state_is_not_serialized_into_savestate() {
    let rom = make_test_rom();
    let mut plain = NES::new();
    plain.load_cartridge_ines(&rom).unwrap();
    plain.reset();
    plain.execute(CoreCommand::SetControllerState {
        port: 0,
        state: ControllerState::from_bits(0xFF),
    });
    plain.execute(CoreCommand::RunFrame);

    let mut playing = NES::new();
    playing.load_cartridge_ines(&rom).unwrap();
    playing.reset();
    playing.execute(CoreCommand::PlayMovie(Movie {
        frames: vec![frame(0xFF, 0x00, 0); 5],
        ..Movie::default()
    }));
    playing.execute(CoreCommand::RunFrame);

    // 两边跑了一帧、同样的手柄输入；播放器状态不应进入存档。
    assert_eq!(plain.save_state().unwrap(), playing.save_state().unwrap());
}
