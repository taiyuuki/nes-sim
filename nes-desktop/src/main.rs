use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};
use nes_sim::video::{VideoBuffer, frame_to_argb32_into};
use nes_sim::{
    ControllerButton, ControllerState, CoreCommand, ExecutionTarget, FdsDiskCommand, FrontendInput,
    FrontendRuntime, RunMode, TVSystem, is_fds_image,
};
use std::collections::VecDeque;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const AUDIO_TARGET_BUFFER_MS: usize = 50;
const AUDIO_MAX_BUFFER_MS: usize = 200;
const AUDIO_CATCHUP_MAX_FRAMES: usize = 5;
/// 回退快照采集间隔（帧）：约0.5秒一份
const REWIND_INTERVAL_FRAMES: u32 = 30;
/// 回退缓冲容量（份）：约30秒历史
const REWIND_HISTORY_LEN: usize = 60;

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
    eprintln!("Usage: {program} [--tv-system auto|ntsc|pal|dendy] [rom-path]");
    eprintln!(r#"Example: {program} --tv-system ntsc "roms/mmc1/Rockman2(J).nes""#);
    eprintln!("说明: 如果不指定 ROM 路径，运行后会弹出文件选择对话框");
    eprintln!("Controls:");
    eprintln!("  P1: Arrows D-pad | Z/X B/A | Tab Select | Enter Start");
    eprintln!("  P2: IJKL D-pad | Comma/Period B/A | RShift Select | RCtrl Start");
    eprintln!("  P       Pause/Resume");
    eprintln!("  N       Step frame");
    eprintln!("  M       Step CPU instruction");
    eprintln!("  R       Reset");
    eprintln!("  Space   Fast-forward (hold)");
    eprintln!("  Backspace Rewind (hold)");
    eprintln!("  1..5    Toggle APU mute (P1/P2/TRI/NOI/DMC)");
    eprintln!("  0       Clear APU mute");
    eprintln!("  [ / ]   Select save slot 1-9");
    eprintln!("  F5      Save state (current slot)");
    eprintln!("  F8      Load state (current slot)");
    eprintln!("  F1      FDS: 插入/退出磁盘");
    eprintln!("  F2      FDS: 切换盘面(需先退盘)");
    eprintln!("  F3      Load cheat file (.cht, FCEUX/Nestopia)");
    eprintln!("  F4      Toggle all cheats on/off");
    eprintln!("  F6      Start/stop FM2 recording (restarts from power-on)");
    eprintln!("  F7      Play FM2 movie (restarts from power-on)");
    eprintln!("  O       Open ROM file");
    eprintln!("  Esc     Quit");
}

/// 按扩展名/魔数加载ROM；FDS镜像自动查找BIOS并优先加载 <rom>.sav 备份，
/// 普通卡带带电池时自动载入 <rom>.sav 存档。`tv_override` 覆盖卡带头部的制式。
fn load_rom_runtime(
    rom_path: &str,
    tv_override: Option<TVSystem>,
) -> Result<FrontendRuntime, String> {
    let rom = std::fs::read(rom_path).map_err(|e| format!("读取文件失败: {e}"))?;
    let mut runtime = if !is_fds_image(&rom) {
        let mut runtime =
            FrontendRuntime::from_rom_bytes(&rom).map_err(|e| format!("加载 ROM 失败: {e}"))?;
        load_battery_save(&mut runtime, rom_path);
        runtime
    } else {
        // FDS: 优先使用写盘备份(.sav), 其次原镜像
        let sav_path = PathBuf::from(rom_path).with_extension("sav");
        let (image, image_path) = match std::fs::read(&sav_path) {
            Ok(sav) if is_fds_image(&sav) => (sav, sav_path.to_string_lossy().to_string()),
            _ => (rom, rom_path.to_string()),
        };

        let bios = find_fds_bios(rom_path)?;
        let runtime = FrontendRuntime::from_rom_bytes_with_bios(&image, Some(&bios))
            .map_err(|e| format!("加载 FDS 失败: {e}"))?;
        eprintln!("FDS: 使用镜像 {image_path} + BIOS ({} 字节)", bios.len());
        runtime
    };
    if let Some(tv) = tv_override {
        runtime.nes_mut().set_tv_system(tv);
        eprintln!("TV system override: {tv:?}");
    }
    Ok(runtime)
}

/// 卡带带电池时载入 <rom>.sav 存档（文件存在才生效）。
fn load_battery_save(runtime: &mut FrontendRuntime, rom_path: &str) {
    if runtime.nes().battery_ram().is_none() {
        return;
    }
    let sav_path = PathBuf::from(rom_path).with_extension("sav");
    if let Ok(data) = std::fs::read(&sav_path) {
        runtime.nes_mut().load_battery_ram(&data);
        eprintln!("电池存档已载入: {}", sav_path.display());
    }
}

/// 查找FDS BIOS ROM (disksys.rom): ROM同目录、BIOS Files子目录、上级目录。
fn find_fds_bios(rom_path: &str) -> Result<Vec<u8>, String> {
    let rom_dir = PathBuf::from(rom_path)
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_default();
    let candidates = [
        rom_dir.join("BIOS Files").join("DISKSYS.ROM"),
        rom_dir.join("disksys.rom"),
        rom_dir.join("disksys.rom.bin"),
        rom_dir
            .parent()
            .map(|p| p.join("disksys.rom"))
            .unwrap_or_default(),
    ];
    for candidate in &candidates {
        if let Ok(bios) = std::fs::read(candidate)
            && bios.len() == 8192
        {
            eprintln!("FDS BIOS: {}", candidate.display());
            return Ok(bios);
        }
    }
    // 找不到则弹出文件选择对话框
    eprintln!("未找到 FDS BIOS (disksys.rom)，请选择...");
    match rfd::FileDialog::new()
        .add_filter("FDS BIOS", &["rom", "bin"])
        .set_title("选择 FDS BIOS (disksys.rom, 8192 字节)")
        .pick_file()
    {
        Some(path) => std::fs::read(&path).map_err(|e| format!("读取 BIOS 失败: {e}")),
        None => Err("未选择 FDS BIOS".to_string()),
    }
}

/// 退出前/换卡前把游戏写入过的数据保存到 <rom>.sav：
/// FDS 磁盘（被写入过才保存）和电池 SRAM。
fn save_persist_data(runtime: &FrontendRuntime, rom_path: &str) {
    let nes = runtime.nes();
    if nes.fds_dirty()
        && let Some(sides) = nes.fds_sides()
    {
        let sav_path = PathBuf::from(rom_path).with_extension("sav");
        let mut data = Vec::with_capacity(sides.len() * 65500);
        for side in sides {
            data.extend_from_slice(side);
        }
        match std::fs::write(&sav_path, &data) {
            Ok(()) => eprintln!("FDS 磁盘已保存: {}", sav_path.display()),
            Err(error) => eprintln!("FDS 磁盘保存失败: {error}"),
        }
    }
    if let Some(ram) = nes.battery_ram() {
        let sav_path = PathBuf::from(rom_path).with_extension("sav");
        match std::fs::write(&sav_path, ram) {
            Ok(()) => eprintln!(
                "电池存档已保存: {} ({} 字节)",
                sav_path.display(),
                ram.len()
            ),
            Err(error) => eprintln!("电池存档保存失败: {error}"),
        }
    }
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
    let mut tv_override: Option<TVSystem> = None;

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
                match parse_tv_system_override(&value) {
                    // "auto"（None）沿用卡带头部
                    Some(Some(tv)) => tv_override = Some(tv),
                    Some(None) => {}
                    None => {
                        eprintln!("invalid --tv-system value {value:?}");
                        usage(&program);
                        return ExitCode::from(2);
                    }
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

    let rom_path = match rom_path {
        Some(path) => path,
        None => {
            eprintln!("未指定 ROM 文件，打开文件选择对话框...");
            match rfd::FileDialog::new()
                .add_filter("NES ROM", &["nes"])
                .add_filter("FDS Disk", &["fds"])
                .add_filter("All Files", &["*"])
                .pick_file()
            {
                Some(path) => {
                    let path_str = path.to_string_lossy().to_string();
                    eprintln!("已选择: {}", path_str);
                    path_str
                }
                None => {
                    eprintln!("未选择文件");
                    return ExitCode::SUCCESS;
                }
            }
        }
    };

    let mut runtime = match load_rom_runtime(&rom_path, tv_override) {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("{error} ({rom_path:?})");
            return ExitCode::from(1);
        }
    };
    let mut current_rom_path = rom_path.clone();
    let mut save_slot: u8 = 1;
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
            player.target_queue_len(),
            AUDIO_MAX_BUFFER_MS,
            player.max_queue_samples,
            AUDIO_CATCHUP_MAX_FRAMES,
        );

        // 预填充音频缓冲区
        let input = FrontendInput {
            controller1: ControllerState::new(),
            ..Default::default()
        };
        while player.queue_len() < player.target_queue_len() {
            let snapshot = runtime.step(input);
            player.push_samples(snapshot.audio.samples, snapshot.audio.sample_rate);
        }

        // 现在启动音频播放
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
    let mut status_message = format!("slot {save_slot}");
    let mut apu_mute_mask = runtime.nes().apu_debug_mute_mask();
    // 自动加载与 ROM 同名的 .cht 金手指文件
    if let Some(message) = try_load_sidecar_cheats(&mut runtime, &rom_path) {
        status_message = message;
    }
    let mut movie_playback_active = false;

    let mut snapshot;

    // 预分配视频缓冲区，避免每帧堆分配
    let mut video_buffer = VideoBuffer::new(nes_sim::FRAME_WIDTH * nes_sim::FRAME_HEIGHT);

    let mut frame_period = tv_frame_period(runtime.nes().tv_system());
    let mut next_frame_deadline = Instant::now() + frame_period;
    let mut frame_times = VecDeque::new();
    let frame_time_window = 60; // 采样 60 帧计算平均值

    // 每秒帧时间分解统计
    let mut step_time_acc = Duration::ZERO;
    let mut catchup_time_acc = Duration::ZERO;
    let mut render_time_acc = Duration::ZERO;
    // let mut frame_count_acc = 0u32;

    // 回退缓冲：环形保留REWIND_HISTORY_LEN份savestate，按住Backspace逐份回退
    let mut rewind_buffer: VecDeque<Vec<u8>> = VecDeque::with_capacity(REWIND_HISTORY_LEN);
    let mut frames_since_rewind_snapshot: u32 = 0;

    while window.is_open() && !window.is_key_down(Key::Escape) {
        let frame_start = Instant::now();
        if window.is_key_pressed(Key::F5, KeyRepeat::No) {
            let save_path = slot_save_path(&current_rom_path, save_slot);
            status_message = match runtime.save_state() {
                Ok(bytes) => match std::fs::write(&save_path, bytes) {
                    Ok(()) => format!("saved {} (slot {save_slot})", save_path.display()),
                    Err(error) => format!("save failed: {error}"),
                },
                Err(error) => format!("save failed: {error}"),
            };
        }

        if window.is_key_pressed(Key::F8, KeyRepeat::No) {
            let save_path = slot_save_path(&current_rom_path, save_slot);
            status_message = match std::fs::read(&save_path) {
                Ok(bytes) => match runtime.load_state(&bytes) {
                    Ok(()) => format!("loaded {} (slot {save_slot})", save_path.display()),
                    Err(error) => format!("load failed: {error}"),
                },
                Err(error) => format!("load failed: {error}"),
            };
        }

        // [ / ] 循环切换存档槽 1-9
        if window.is_key_pressed(Key::LeftBracket, KeyRepeat::No) {
            save_slot = if save_slot == 1 { 9 } else { save_slot - 1 };
            status_message = format!("slot {save_slot}");
        }
        if window.is_key_pressed(Key::RightBracket, KeyRepeat::No) {
            save_slot = if save_slot == 9 { 1 } else { save_slot + 1 };
            status_message = format!("slot {save_slot}");
        }

        if handle_apu_debug_hotkeys(&window, &mut runtime, &mut apu_mute_mask) {
            status_message = format!("apu mute {}", apu_mute_mask_to_string(apu_mute_mask));
        }

        // O 键：加载新 ROM
        if window.is_key_pressed(Key::O, KeyRepeat::No) {
            status_message = match prompt_and_load_rom() {
                Ok(new_rom_path) => {
                    // 先保存旧FDS磁盘
                    save_persist_data(&runtime, &current_rom_path);
                    match load_rom_runtime(&new_rom_path, tv_override) {
                        Ok(mut new_runtime) => {
                            // 停止旧音频播放
                            if let Some(player) = &mut audio_player {
                                player.stop_playback();
                            }
                            // 创建新音频播放器
                            audio_player =
                                match AudioPlayer::new(new_runtime.snapshot().audio.sample_rate) {
                                    Ok(player) => {
                                        new_runtime
                                            .nes_mut()
                                            .set_apu_sample_rate(player.output_sample_rate());
                                        Some(player)
                                    }
                                    Err(error) => {
                                        eprintln!("音频初始化失败: {error}");
                                        None
                                    }
                                };
                            // 预填充音频缓冲区
                            if let Some(player) = &mut audio_player {
                                let input = FrontendInput {
                                    controller1: ControllerState::new(),
                                    ..Default::default()
                                };
                                while player.queue_len() < player.target_queue_len() {
                                    let snap = new_runtime.step(input);
                                    player.push_samples(snap.audio.samples, snap.audio.sample_rate);
                                }
                                player.start_playback();
                            }
                            // 更新状态
                            runtime = new_runtime;
                            current_rom_path = new_rom_path.clone();
                            frame_period = tv_frame_period(runtime.nes().tv_system());
                            next_frame_deadline = Instant::now() + frame_period;
                            rewind_buffer.clear();
                            frames_since_rewind_snapshot = 0;
                            let new_apu_mute_mask = runtime.nes().apu_debug_mute_mask();
                            apu_mute_mask = new_apu_mute_mask;
                            // 新 ROM：尝试自动加载同名 .cht
                            let sidecar = try_load_sidecar_cheats(&mut runtime, &current_rom_path);
                            // 返回新状态信息
                            format!(
                                "已加载: {}",
                                PathBuf::from(&new_rom_path)
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                            ) + &sidecar.map_or(String::new(), |m| format!(" | {m}"))
                        }
                        Err(error) => format!("加载 ROM 失败: {error}"),
                    }
                }
                Err(msg) => msg,
            };
        }

        // FDS磁盘热键
        if window.is_key_pressed(Key::F1, KeyRepeat::No) {
            runtime.nes_mut().fds_command(FdsDiskCommand::ToggleInsert);
            status_message = fds_status_message(&runtime);
        }
        if window.is_key_pressed(Key::F2, KeyRepeat::No) {
            runtime
                .nes_mut()
                .fds_command(FdsDiskCommand::SelectNextSide);
            status_message = fds_status_message(&runtime);
        }

        // F3：加载金手指文件（FCEUX 文本 / Nestopia/Mesen XML 自动识别）
        if window.is_key_pressed(Key::F3, KeyRepeat::No) {
            status_message = match rfd::FileDialog::new()
                .add_filter("Cheat File", &["cht"])
                .add_filter("All Files", &["*"])
                .pick_file()
            {
                Some(path) => {
                    match std::fs::read(&path)
                        .map_err(|e| e.to_string())
                        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                        .and_then(|text| {
                            nes_sim::cheat::parse_cht_file(&text).map_err(|e| e.to_string())
                        }) {
                        Ok(cheats) => {
                            let warning =
                                cheat_game_mismatch_warning(&cheats, runtime.nes().rom_crc32());
                            let message =
                                cheats_status_message(&cheats) + warning.as_deref().unwrap_or("");
                            runtime.nes_mut().execute(CoreCommand::SetCheats(cheats));
                            message
                        }
                        Err(error) => format!("cheat load failed: {error}"),
                    }
                }
                None => "cheat load cancelled".to_string(),
            };
        }

        // F4：金手指总开关（全部启用/全部禁用）
        if window.is_key_pressed(Key::F4, KeyRepeat::No) {
            let mut cheats = runtime.nes().cheats().to_vec();
            if cheats.is_empty() {
                status_message = "no cheats loaded".to_string();
            } else {
                let disable = cheats.iter().any(|c| c.enabled);
                for cheat in &mut cheats {
                    cheat.enabled = !disable;
                }
                status_message = cheats_status_message(&cheats);
                runtime.nes_mut().execute(CoreCommand::SetCheats(cheats));
            }
        }

        // F6：开始/停止录像（停止时弹保存对话框）
        if window.is_key_pressed(Key::F6, KeyRepeat::No) {
            if runtime.nes().is_recording_movie() {
                runtime.nes_mut().execute(CoreCommand::StopMovieRecording);
                status_message = match runtime.nes_mut().take_recorded_movie() {
                    Some(mut movie) => {
                        // 前端补全核心不知道的元数据（ROM 文件名）
                        if movie.rom_filename.is_empty() {
                            movie.rom_filename = PathBuf::from(&current_rom_path)
                                .file_stem()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default();
                        }
                        match prompt_save_movie(&current_rom_path, &movie) {
                            Ok(path) => format!("movie saved: {path}"),
                            Err(error) => format!("movie save failed: {error}"),
                        }
                    }
                    None => "no movie recorded".to_string(),
                };
            } else {
                // FM2 只支持从上电开始录像：复位到近似上电态，保证产出可独立回放
                runtime.nes_mut().power_cycle();
                runtime.nes_mut().execute(CoreCommand::StartMovieRecording);
                movie_playback_active = false;
                rewind_buffer.clear();
                frames_since_rewind_snapshot = 0;
                status_message = "recording from power-on... (F6 to stop & save)".to_string();
            }
        }

        // F7：播放 FM2 录像（从近似上电状态开始）
        if window.is_key_pressed(Key::F7, KeyRepeat::No) {
            if runtime.nes().is_recording_movie() {
                status_message = "stop recording first (F6)".to_string();
            } else {
                status_message = match rfd::FileDialog::new()
                    .add_filter("FM2 Movie", &["fm2"])
                    .add_filter("All Files", &["*"])
                    .pick_file()
                {
                    Some(path) => {
                        match std::fs::read(&path)
                            .map_err(|e| e.to_string())
                            .and_then(|bytes| {
                                nes_sim::movie::parse_fm2(&bytes).map_err(|e| e.to_string())
                            }) {
                            Ok(movie) => {
                                let frames = movie.frames.len();
                                let rom_name = if movie.rom_filename.is_empty() {
                                    path.to_string_lossy().to_string()
                                } else {
                                    movie.rom_filename.clone()
                                };
                                // romChecksum（MD5，PRG‖CHR 口径）不匹配时警告但仍可播放
                                let mismatch =
                                    rom_checksum_warning(&movie, runtime.nes().rom_md5_hex());
                                runtime.nes_mut().power_cycle();
                                runtime.nes_mut().execute(CoreCommand::PlayMovie(movie));
                                movie_playback_active = true;
                                rewind_buffer.clear();
                                frames_since_rewind_snapshot = 0;
                                format!("playing {frames} frames: {rom_name}")
                                    + mismatch.as_deref().unwrap_or("")
                            }
                            Err(error) => format!("movie load failed: {error}"),
                        }
                    }
                    None => "movie load cancelled".to_string(),
                };
            }
        }

        // 录像播放结束检测（FrontendRuntime 不透传事件，轮询进度）
        if movie_playback_active
            && runtime
                .nes()
                .movie()
                .is_some_and(|m| runtime.nes().movie_frame() == Some(m.frames.len()))
        {
            let frames = runtime.nes().movie().map_or(0, |m| m.frames.len());
            runtime.nes_mut().execute(CoreCommand::StopMovie);
            movie_playback_active = false;
            status_message = format!("movie finished ({frames} frames)");
        }

        let input = collect_input(&window);
        let fast_forward = window.is_key_down(Key::Space);
        // 按住Backspace回退；缓冲耗尽时按正常速度前进，避免空转。
        // 录像中禁用回退（倒带会破坏录像帧序）
        let rewinding = window.is_key_down(Key::Backspace)
            && !rewind_buffer.is_empty()
            && !runtime.nes().is_recording_movie();
        let t_step = Instant::now();
        if rewinding {
            if let Some(state) = rewind_buffer.pop_back()
                && let Err(error) = runtime.nes_mut().load_state(&state)
            {
                status_message = format!("rewind failed: {error}");
            }
            snapshot = runtime.snapshot();
            step_time_acc += t_step.elapsed();
        } else {
            let status = runtime.step(input).status;
            step_time_acc += t_step.elapsed();

            // 实际执行了帧才采集回退快照（暂停/单步时跳过）
            if matches!(status.executed, ExecutionTarget::Frame) {
                frames_since_rewind_snapshot += 1;
                if frames_since_rewind_snapshot >= REWIND_INTERVAL_FRAMES {
                    frames_since_rewind_snapshot = 0;
                    if let Ok(state) = runtime.save_state() {
                        rewind_buffer.push_back(state);
                        while rewind_buffer.len() > REWIND_HISTORY_LEN {
                            rewind_buffer.pop_front();
                        }
                    }
                }
            }
            snapshot = runtime.snapshot();
            if status.quit_requested {
                break;
            }
        }

        let t_catchup = Instant::now();
        if fast_forward || rewinding {
            // 快进/回退期间停用音频（清空队列），松开后由常规路径自动恢复播放
            if let Some(player) = &mut audio_player {
                player.stop_playback();
            }
        } else if let Some(player) = &mut audio_player {
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
        if fast_forward || rewinding {
            // 快进/回退不等待；保持deadline有效，松开后立即恢复正常节奏
            next_frame_deadline = now + frame_period;
        } else if frame_elapsed < frame_period {
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

    // 退出前保存FDS磁盘写入
    save_persist_data(&runtime, &current_rom_path);

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

/// 按制式的视频帧周期：NTSC 60.0988fps，PAL/Dendy 50.0070fps。
fn tv_frame_period(tv: TVSystem) -> Duration {
    match tv {
        TVSystem::NTSC => Duration::from_micros(16_639),
        TVSystem::PAL | TVSystem::DENDY => Duration::from_micros(19_997),
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
    underrun_last_report: Instant,
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

    fn output_sample_rate(&self) -> u32 {
        self.output_sample_rate
    }

    fn target_queue_len(&self) -> usize {
        self.target_queue_samples
    }

    fn start_playback(&mut self) {
        if !self.playback_started {
            if let Err(e) = self.stream.play() {
                eprintln!("failed to start audio stream: {e}");
            }
            self.playback_started = true;
        }
    }

    fn stop_playback(&mut self) {
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

    fn queue_len(&self) -> usize {
        self.output_state.lock().map_or(0, |s| s.queue.len())
    }

    fn push_samples(&self, samples: &[f32], source_sample_rate: u32) {
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

/// 单个手柄的键位映射。
struct KeyMapping {
    a: Key,
    b: Key,
    select: Key,
    start: Key,
    up: Key,
    down: Key,
    left: Key,
    right: Key,
}

/// P1: 方向键 + Z/X + Tab/Enter；P2: IJKL + ,/. + RightShift/RightCtrl
const P1_KEYS: KeyMapping = KeyMapping {
    a: Key::X,
    b: Key::Z,
    select: Key::Tab,
    start: Key::Enter,
    up: Key::Up,
    down: Key::Down,
    left: Key::Left,
    right: Key::Right,
};

const P2_KEYS: KeyMapping = KeyMapping {
    a: Key::Period,
    b: Key::Comma,
    select: Key::RightShift,
    start: Key::RightCtrl,
    up: Key::I,
    down: Key::K,
    left: Key::J,
    right: Key::L,
};

fn collect_controller(window: &Window, keys: &KeyMapping) -> ControllerState {
    let mut state = ControllerState::new();
    set_button(&mut state, window.is_key_down(keys.a), ControllerButton::A);
    set_button(&mut state, window.is_key_down(keys.b), ControllerButton::B);
    set_button(
        &mut state,
        window.is_key_down(keys.select),
        ControllerButton::Select,
    );
    set_button(
        &mut state,
        window.is_key_down(keys.start),
        ControllerButton::Start,
    );
    set_button(
        &mut state,
        window.is_key_down(keys.up),
        ControllerButton::Up,
    );
    set_button(
        &mut state,
        window.is_key_down(keys.down),
        ControllerButton::Down,
    );
    set_button(
        &mut state,
        window.is_key_down(keys.left),
        ControllerButton::Left,
    );
    set_button(
        &mut state,
        window.is_key_down(keys.right),
        ControllerButton::Right,
    );
    state
}

fn collect_input(window: &Window) -> FrontendInput {
    FrontendInput {
        controller1: collect_controller(window, &P1_KEYS),
        controller2: collect_controller(window, &P2_KEYS),
        reset: window.is_key_pressed(Key::R, KeyRepeat::No),
        toggle_pause: window.is_key_pressed(Key::P, KeyRepeat::No),
        step_frame: window.is_key_pressed(Key::N, KeyRepeat::No),
        step_cpu_instruction: window.is_key_pressed(Key::M, KeyRepeat::No),
        quit: window.is_key_pressed(Key::Escape, KeyRepeat::No),
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

/// FDS驱动器状态描述（非FDS卡带给出提示）。
fn fds_status_message(runtime: &FrontendRuntime) -> String {
    match runtime.nes().fds_info() {
        Some(info) => format!(
            "FDS: {}面 | {} | 选中面 {} (盘{} {})",
            info.side_count,
            if info.inserted {
                "已插入"
            } else {
                "已退盘"
            },
            info.selected_side,
            info.selected_side / 2,
            if info.selected_side % 2 == 0 {
                "A"
            } else {
                "B"
            }
        ),
        None => "非FDS卡带".to_string(),
    }
}

/// 存档槽对应的即时存档文件： <rom>.state<slot>（槽位1-9）。
fn slot_save_path(rom_path: &str, slot: u8) -> PathBuf {
    PathBuf::from(rom_path).with_extension(format!("state{slot}"))
}

/// 金手指状态消息："cheats 启用数/总数"。
fn cheats_status_message(cheats: &[nes_sim::Cheat]) -> String {
    let total = cheats.len();
    let enabled = cheats.iter().filter(|c| c.enabled).count();
    format!("cheats {enabled}/{total} active")
}

/// Nestopia .cht 的 game 属性（CRC32）与当前 ROM 不匹配的条数提示。
fn cheat_game_mismatch_warning(
    cheats: &[nes_sim::Cheat],
    rom_crc32: Option<u32>,
) -> Option<String> {
    let rom_crc32 = rom_crc32?;
    let mismatched = cheats
        .iter()
        .filter(|c| c.game_crc.is_some_and(|crc| crc != rom_crc32))
        .count();
    (mismatched > 0).then(|| format!(" [warning: {mismatched} cheats bound to another ROM]"))
}

/// fm2 的 romChecksum（MD5 hex）与当前 ROM 不匹配时返回警告后缀。
fn rom_checksum_warning(movie: &nes_sim::Movie, rom_md5_hex: Option<String>) -> Option<String> {
    let actual = rom_md5_hex?;
    (!movie.rom_checksum.is_empty() && !movie.rom_checksum.eq_ignore_ascii_case(&actual))
        .then(|| " [warning: ROM mismatch!]".to_string())
}

/// 自动加载与 ROM 同名的 .cht 金手指文件（FCEUX 习惯）。
/// 文件不存在时静默返回 None；存在则返回状态消息（含解析失败提示）。
fn try_load_sidecar_cheats(runtime: &mut FrontendRuntime, rom_path: &str) -> Option<String> {
    let path = PathBuf::from(rom_path).with_extension("cht");
    let text = String::from_utf8_lossy(&std::fs::read(&path).ok()?).into_owned();
    match nes_sim::cheat::parse_cht_file(&text) {
        Ok(cheats) => {
            let message = cheats_status_message(&cheats);
            runtime.nes_mut().execute(CoreCommand::SetCheats(cheats));
            Some(format!("{message}: {}", path.display()))
        }
        Err(error) => Some(format!("cheat load failed: {error} ({})", path.display())),
    }
}

/// 弹出保存对话框写入 FM2 录像；默认文件名 <rom>.fm2。
fn prompt_save_movie(rom_path: &str, movie: &nes_sim::Movie) -> Result<String, String> {
    let default_name = format!(
        "{}.fm2",
        PathBuf::from(rom_path)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "movie".to_string())
    );
    let path = rfd::FileDialog::new()
        .add_filter("FM2 Movie", &["fm2"])
        .set_file_name(&default_name)
        .save_file()
        .ok_or_else(|| "cancelled".to_string())?;
    let path_str = path.to_string_lossy().to_string();
    std::fs::write(&path, movie.to_fm2_string()).map_err(|e| format!("写入失败: {e}"))?;
    Ok(path_str)
}

/// 弹出文件选择对话框并尝试加载 ROM
/// 返回 Ok(新 rom 路径) 或 Err(错误信息)
fn prompt_and_load_rom() -> Result<String, String> {
    match rfd::FileDialog::new()
        .add_filter("NES ROM", &["nes"])
        .add_filter("FDS Disk", &["fds"])
        .add_filter("All Files", &["*"])
        .pick_file()
    {
        Some(path) => {
            let path_str = path.to_string_lossy().to_string();
            // 验证文件可以读取
            std::fs::read(&path_str)
                .map(|_| path_str)
                .map_err(|error| format!("无法读取文件: {error}"))
        }
        None => Err("用户取消选择".to_string()),
    }
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

#[cfg(test)]
mod tests;
