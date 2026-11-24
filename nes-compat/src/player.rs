//! 模拟器播放线程：独占 `FrontendRuntime` 与 `AudioPlayer`，
//! UI 线程通过共享状态送入输入、取出 RGBA 帧与状态信息。

use nes_audio::AudioPlayer;
use nes_sim::video::Palette;
use nes_sim::{
    ControllerState, FRAME_HEIGHT, FRAME_WIDTH, FdsDiskCommand, FrontendInput, FrontendRuntime,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// 一次播放请求：ROM/NSF 字节 + 元数据。
pub struct PlayRequest {
    pub bytes: Vec<u8>,
    /// 文件名（错误提示用）
    pub fname: String,
    /// 展示名（游戏名 · 文件名）
    pub display: String,
    /// FDS BIOS（可空）
    pub fds_bios: Option<Vec<u8>>,
}

enum PlayerCmd {
    Load(PlayRequest),
    /// 卸载当前内容（线程保持存活）
    Stop,
}

/// UI 线程写入、模拟器线程每帧取走并清零边沿的输入。
#[derive(Default)]
pub struct InputState {
    pub p1: ControllerState,
    pub p2: ControllerState,
    pub reset: bool,
    pub toggle_pause: bool,
    pub fds_toggle_insert: bool,
    pub fds_next_side: bool,
    pub nsf_track_delta: i32,
}

/// 模拟器线程产出的最新帧与状态。
pub struct FrameState {
    pub ready: bool,
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
    pub frame_no: u64,
    pub fps: f32,
    pub paused: bool,
    pub tv_system: String,
    pub nsf_track: Option<(u16, u16)>,
    pub rom_display: String,
}

impl Default for FrameState {
    fn default() -> Self {
        Self {
            ready: false,
            width: FRAME_WIDTH,
            height: FRAME_HEIGHT,
            rgba: vec![0; FRAME_WIDTH * FRAME_HEIGHT * 4],
            frame_no: 0,
            fps: 0.0,
            paused: false,
            tv_system: String::new(),
            nsf_track: None,
            rom_display: String::new(),
        }
    }
}

struct Shared {
    input: Mutex<InputState>,
    frame: Mutex<FrameState>,
    fast_forward: AtomicBool,
    running: AtomicBool,
    /// 加载失败/运行时错误提示（UI 读取展示）
    message: Mutex<String>,
}

#[derive(Clone)]
pub struct PlayerHandle {
    shared: Arc<Shared>,
    cmd_tx: mpsc::Sender<PlayerCmd>,
}

impl PlayerHandle {
    /// 启动播放线程（整个应用生命周期复用）。
    pub fn spawn(ctx: egui::Context) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<PlayerCmd>();
        let shared = Arc::new(Shared {
            input: Mutex::new(InputState::default()),
            frame: Mutex::new(FrameState::default()),
            fast_forward: AtomicBool::new(false),
            running: AtomicBool::new(false),
            message: Mutex::new(String::new()),
        });
        let thread_shared = Arc::clone(&shared);
        let builder = thread::Builder::new().name("nes-player".into());
        let _join: JoinHandle<()> = builder
            .spawn(move || player_loop(thread_shared, cmd_rx, ctx))
            .expect("spawn player thread");
        Self { shared, cmd_tx }
    }

    pub fn load(&self, request: PlayRequest) {
        let _ = self.cmd_tx.send(PlayerCmd::Load(request));
    }

    pub fn stop(&self) {
        let _ = self.cmd_tx.send(PlayerCmd::Stop);
    }

    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::Relaxed)
    }

    pub fn set_fast_forward(&self, on: bool) {
        self.shared.fast_forward.store(on, Ordering::Relaxed);
    }

    /// 独占输入状态一帧（读取后清零边沿）。
    pub fn with_input<R>(&self, f: impl FnOnce(&mut InputState) -> R) -> R {
        let mut input = self.shared.input.lock().unwrap();
        f(&mut input)
    }

    pub fn snapshot_frame(&self) -> FrameState {
        self.shared.frame.lock().unwrap().clone_frame()
    }

    pub fn message(&self) -> String {
        self.shared.message.lock().unwrap().clone()
    }

    /// 外部（如解压线程）写入提示信息。
    pub fn set_message(&self, message: impl Into<String>) {
        *self.shared.message.lock().unwrap() = message.into();
    }

    /// 当前 fast_forward 标志（用于 UI 显示）。
    pub fn fast_forward(&self) -> bool {
        self.shared.fast_forward.load(Ordering::Relaxed)
    }
}

impl FrameState {
    /// 供跨线程读取的浅拷贝（rgba 缓冲克隆）。
    fn clone_frame(&self) -> Self {
        Self {
            ready: self.ready,
            width: self.width,
            height: self.height,
            rgba: self.rgba.clone(),
            frame_no: self.frame_no,
            fps: self.fps,
            paused: self.paused,
            tv_system: self.tv_system.clone(),
            nsf_track: self.nsf_track,
            rom_display: self.rom_display.clone(),
        }
    }
}

/// 音频追赶上限：每帧最多补跑帧数（同 nes-desktop）。
const AUDIO_CATCHUP_MAX_FRAMES: usize = 5;

fn player_loop(shared: Arc<Shared>, cmd_rx: mpsc::Receiver<PlayerCmd>, ctx: egui::Context) {
    let mut runtime: Option<FrontendRuntime> = Option::None;
    let mut audio: Option<AudioPlayer> = Option::None;
    let palette = Palette::DEFAULT;
    let mut rgba_buf: Vec<u8> = Vec::with_capacity(FRAME_WIDTH * FRAME_HEIGHT * 4);
    let mut next_deadline = Instant::now();
    let mut fast_forward_prev = false;
    // fps 统计
    let mut frames_in_window = 0u32;
    let mut fps_window_start = Instant::now();
    let mut fps = 0.0f32;

    loop {
        // 处理命令（非阻塞；命令到来前可能积压多条）
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                PlayerCmd::Load(request) => {
                    let mut runtime_new = match FrontendRuntime::from_rom_bytes_with_bios(
                        &request.bytes,
                        request.fds_bios.as_deref(),
                    ) {
                        Ok(runtime) => runtime,
                        Err(error) => {
                            runtime = None;
                            shared.running.store(false, Ordering::Relaxed);
                            *shared.message.lock().unwrap() =
                                format!("加载失败: {error}（{}）", request.fname);
                            if let Some(player) = &mut audio {
                                player.stop_playback();
                            }
                            update_frame_stub(&shared);
                            ctx.request_repaint();
                            continue;
                        }
                    };
                    if let Some(player) = &mut audio {
                        player.stop_playback();
                    }
                    let snapshot = runtime_new.snapshot();
                    // 建立音频通道
                    audio = match AudioPlayer::new(snapshot.audio.sample_rate) {
                        Ok(mut player) => {
                            runtime_new
                                .nes_mut()
                                .set_apu_sample_rate(player.output_sample_rate());
                            // 预填充后再开播
                            let input = FrontendInput::default();
                            while player.queue_len() < player.target_queue_len() {
                                let snap = runtime_new.step(input);
                                player.push_samples(snap.audio.samples, snap.audio.sample_rate);
                            }
                            player.start_playback();
                            Some(player)
                        }
                        Err(error) => {
                            eprintln!("audio disabled: {error}");
                            None
                        }
                    };
                    let tv = format!("{:?}", runtime_new.nes().tv_system());
                    if let Some(info) = runtime_new.nes().nsf_info() {
                        eprintln!(
                            "NSF: {} / {} / {} — {} 曲",
                            info.name, info.artist, info.copyright, info.track_count
                        );
                    }
                    {
                        let mut frame = shared.frame.lock().unwrap();
                        frame.ready = true;
                        frame.frame_no = 0;
                        frame.paused = false;
                        frame.tv_system = tv;
                        frame.rom_display = request.display;
                    }
                    shared.running.store(true, Ordering::Relaxed);
                    *shared.message.lock().unwrap() = String::new();
                    runtime = Some(runtime_new);
                    next_deadline = Instant::now();
                }
                PlayerCmd::Stop => {
                    runtime = None;
                    shared.running.store(false, Ordering::Relaxed);
                    if let Some(player) = &mut audio {
                        player.stop_playback();
                    }
                    update_frame_stub(&shared);
                }
            }
        }

        let Some(runtime) = runtime.as_mut() else {
            // 空闲：低频轮询命令
            thread::sleep(Duration::from_millis(30));
            continue;
        };

        // 取走本帧输入（边沿标志随取随清）
        let input_state = {
            let mut input = shared.input.lock().unwrap();
            std::mem::take(&mut *input)
        };
        let fast_forward = shared.fast_forward.load(Ordering::Relaxed);
        let input = FrontendInput {
            controller1: input_state.p1,
            controller2: input_state.p2,
            reset: input_state.reset,
            toggle_pause: input_state.toggle_pause,
            step_frame: false,
            step_cpu_instruction: false,
            quit: false,
        };

        let snapshot = runtime.step(input);
        // 立即消费快照（视频转换 + 音频入队），解除对 runtime 的借用
        let mode_running = matches!(snapshot.status.mode, nes_sim::RunMode::Running);
        let frame_paused = matches!(snapshot.status.mode, nes_sim::RunMode::Paused);
        let frame_no = snapshot.video.frame_number;
        convert_rgba(snapshot.video, &palette, &mut rgba_buf);
        if !fast_forward && let Some(player) = audio.as_ref() {
            player.push_samples(snapshot.audio.samples, snapshot.audio.sample_rate);
        }

        // 边沿命令（FDS / NSF 切曲）
        if input_state.fds_toggle_insert {
            runtime.nes_mut().fds_command(FdsDiskCommand::ToggleInsert);
        }
        if input_state.fds_next_side {
            runtime
                .nes_mut()
                .fds_command(FdsDiskCommand::SelectNextSide);
        }
        if input_state.nsf_track_delta != 0
            && let Some(info) = runtime.nes().nsf_info()
        {
            let count = info.track_count as i32;
            let mut next = info.current_track as i32 + input_state.nsf_track_delta;
            if next < 1 {
                next = count;
            }
            if next > count {
                next = 1;
            }
            runtime.nes_mut().nsf_select_track(next as u16);
        }

        // 快进切换：停用/恢复音频
        if fast_forward != fast_forward_prev {
            if let Some(player) = &mut audio {
                if fast_forward {
                    player.stop_playback();
                } else {
                    player.start_playback();
                }
            }
            fast_forward_prev = fast_forward;
        }

        // 音频追赶（队列水位低时补跑数帧）
        if !fast_forward
            && mode_running
            && let Some(player) = audio.as_mut()
        {
            if !player.playback_started() {
                player.start_playback();
            }
            let catch_input = FrontendInput {
                controller1: input_state.p1,
                controller2: input_state.p2,
                ..FrontendInput::default()
            };
            let mut catch_up = 0;
            while player.is_queue_low() && catch_up < AUDIO_CATCHUP_MAX_FRAMES {
                let snap = runtime.step(catch_input);
                player.push_samples(snap.audio.samples, snap.audio.sample_rate);
                catch_up += 1;
            }
        }

        // 视频帧发布
        let nsf_track = runtime
            .nes()
            .nsf_info()
            .map(|info| (info.current_track, info.track_count));
        {
            let mut frame = shared.frame.lock().unwrap();
            frame.ready = true;
            frame.width = nes_sim::FRAME_WIDTH;
            frame.height = nes_sim::FRAME_HEIGHT;
            if frame.rgba.len() == rgba_buf.len() {
                frame.rgba.clone_from(&rgba_buf);
            } else {
                frame.rgba = rgba_buf.clone();
            }
            frame.frame_no = frame_no;
            frame.paused = frame_paused;
            frame.nsf_track = nsf_track;
            frame.fps = fps;
        }

        // 帧率统计
        frames_in_window += 1;
        let elapsed = fps_window_start.elapsed();
        if elapsed >= Duration::from_secs(1) {
            fps = frames_in_window as f32 / elapsed.as_secs_f32();
            frames_in_window = 0;
            fps_window_start = Instant::now();
        }

        ctx.request_repaint();

        // 节奏控制：NTSC 16.639ms / PAL·Dendy 19.997ms
        if !fast_forward {
            let period = match runtime.nes().tv_system() {
                nes_sim::TVSystem::NTSC => Duration::from_micros(16_639),
                nes_sim::TVSystem::PAL | nes_sim::TVSystem::DENDY => Duration::from_micros(19_997),
            };
            next_deadline += period;
            let now = Instant::now();
            if now < next_deadline {
                thread::sleep(next_deadline - now);
            } else {
                next_deadline = now;
            }
        } else {
            next_deadline = Instant::now();
        }
    }
}

fn update_frame_stub(shared: &Shared) {
    let mut frame = shared.frame.lock().unwrap();
    frame.ready = false;
    frame.paused = false;
    frame.frame_no = 0;
}

/// 调色板索引帧 → RGBA8（复用缓冲，避免每帧分配）。
fn convert_rgba(frame: nes_sim::VideoFrame<'_>, palette: &Palette, out: &mut Vec<u8>) {
    out.clear();
    out.reserve(frame.width * frame.height * 4);
    for row in 0..frame.height {
        let start = row * frame.pitch;
        let line = &frame.pixels[start..start + frame.width];
        for &pixel in line {
            let [r, g, b] = palette.lookup(pixel, frame.emphasis);
            out.extend_from_slice(&[r, g, b, 255]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 端到端：加载 roms/ 下的 ROM，播放线程应产出非空 RGBA 帧。
    /// 缺 ROM 环境跳过；无音频设备自动降级为无声播放。
    #[test]
    fn player_thread_loads_and_produces_frames() {
        let rom_path = std::path::Path::new("roms/Spacegulls-1.1.nes");
        let Ok(bytes) = std::fs::read(rom_path) else {
            eprintln!("skip: {rom_path:?} 不可用");
            return;
        };
        let ctx = egui::Context::default();
        let player = PlayerHandle::spawn(ctx);
        player.load(PlayRequest {
            bytes,
            fname: "spacegulls.nes".into(),
            display: "Spacegulls".into(),
            fds_bios: Option::None,
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            let frame = player.snapshot_frame();
            if player.is_running() && frame.frame_no >= 10 {
                assert_eq!(frame.rgba.len(), frame.width * frame.height * 4);
                assert!(frame.rgba.iter().any(|&b| b != 0), "帧不应全黑（开机画面）");
                player.stop();
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("播放线程未在超时内产出帧: {}", player.message());
    }
}
