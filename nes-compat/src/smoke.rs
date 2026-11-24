//! 批量冒烟测试：解压 → 加载 → 无头跑 N 帧 → 记录加载失败/黑屏/冻结等信号。
//! 结果写入 compat.db（人工记录优先，自动结果不覆盖）。

use crate::archive::ArchiveCache;
use crate::compatdb::{self, CompatStatus, ItemKind};
use nes_sim::headless::stable_byte_hash;
use nes_sim::{CartridgeError, FrontendInput, FrontendRuntime};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct SmokeItem {
    /// 结果库 key（整文件 MD5）
    pub key: String,
    pub kind: ItemKind,
    pub game_id: i64,
    pub fname: String,
}

#[derive(Debug, Clone)]
pub struct SmokeOutcome {
    pub key: String,
    pub fname: String,
    pub status: CompatStatus,
    pub detail: String,
    pub frames: u32,
    pub elapsed_ms: u64,
    pub distinct_colors: usize,
    pub audio_energy: f64,
    pub frozen: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SmokeProgress {
    pub done: usize,
    pub total: usize,
    pub current: String,
    pub started_at: Option<Instant>,
    pub finished: bool,
}

/// 黑屏判定：末帧调色板索引去重数 ≤ 2 视为无有效画面。
const BLANK_COLOR_THRESHOLD: usize = 2;
/// NSF 音频能量阈值（平均每样本振幅）。
const AUDIO_SILENT_THRESHOLD: f64 = 1e-5;

/// 一次批量冒烟的运行配置。
pub struct SmokeOptions {
    pub frames: u32,
    pub cache: Arc<ArchiveCache>,
    pub ever_dir: PathBuf,
    pub fds_bios: Option<Vec<u8>>,
    pub compat_db: PathBuf,
}

/// 逐项冒烟。`on_outcome` 在每项完成时回调（GUI 实时刷表 / CLI 打印）。
pub fn run_smoke(
    items: Vec<SmokeItem>,
    options: &SmokeOptions,
    cancel: &AtomicBool,
    progress: Option<&Mutex<SmokeProgress>>,
    mut on_outcome: impl FnMut(&SmokeOutcome),
) -> Vec<SmokeOutcome> {
    let mut outcomes = Vec::new();
    let conn = match compatdb::open(&options.compat_db) {
        Ok(conn) => Some(conn),
        Err(error) => {
            eprintln!("打开结果库失败，冒烟结果不会持久化: {error}");
            None
        }
    };
    if let Some(progress) = progress
        && let Ok(mut p) = progress.lock()
    {
        p.total = items.len();
        p.done = 0;
        p.started_at = Some(Instant::now());
        p.finished = false;
    }
    for item in items {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        if let Some(progress) = progress
            && let Ok(mut p) = progress.lock()
        {
            p.current = item.fname.clone();
        }
        let outcome = smoke_one(
            &item,
            options.frames,
            &options.cache,
            &options.ever_dir,
            options.fds_bios.as_deref(),
        );
        if let Some(conn) = &conn {
            let _ = compatdb::set_result(
                conn,
                &outcome.key,
                item.kind,
                &outcome.fname,
                item.game_id,
                outcome.status,
                &outcome.detail,
            );
            let _ = compatdb::log_smoke(
                conn,
                &outcome.key,
                outcome.frames,
                outcome.elapsed_ms,
                outcome.distinct_colors,
                outcome.audio_energy,
                outcome.frozen,
                &outcome.detail,
            );
        }
        on_outcome(&outcome);
        outcomes.push(outcome);
        if let Some(progress) = progress
            && let Ok(mut p) = progress.lock()
        {
            p.done += 1;
        }
    }
    if let Some(progress) = progress
        && let Ok(mut p) = progress.lock()
    {
        p.finished = true;
        p.current.clear();
    }
    outcomes
}

fn smoke_one(
    item: &SmokeItem,
    frames: u32,
    cache: &ArchiveCache,
    ever_dir: &std::path::Path,
    fds_bios: Option<&[u8]>,
) -> SmokeOutcome {
    let started = Instant::now();
    let mut outcome = SmokeOutcome {
        key: item.key.clone(),
        fname: item.fname.clone(),
        status: CompatStatus::Skip,
        detail: String::new(),
        frames: 0,
        elapsed_ms: 0,
        distinct_colors: 0,
        audio_energy: 0.0,
        frozen: false,
    };
    let nsf = item.kind == ItemKind::Nsf;
    let bytes = match cache.extract_by_name(ever_dir, item.game_id, &item.fname, nsf) {
        Ok(bytes) => bytes,
        Err(error) => {
            outcome.detail = error;
            outcome.elapsed_ms = started.elapsed().as_millis() as u64;
            return outcome;
        }
    };

    let mut runtime = match FrontendRuntime::from_rom_bytes_with_bios(&bytes, fds_bios) {
        Ok(runtime) => runtime,
        Err(CartridgeError::FdsBiosMissing) => {
            outcome.detail = "FDS 需要 disksys.rom（设置中配置）".into();
            outcome.elapsed_ms = started.elapsed().as_millis() as u64;
            return outcome;
        }
        Err(CartridgeError::UnsupportedMapper(mapper)) => {
            outcome.status = CompatStatus::LoadError;
            outcome.detail = format!("mapper {mapper} 未支持");
            outcome.elapsed_ms = started.elapsed().as_millis() as u64;
            return outcome;
        }
        Err(error) => {
            outcome.status = CompatStatus::LoadError;
            outcome.detail = format!("{error}");
            outcome.elapsed_ms = started.elapsed().as_millis() as u64;
            return outcome;
        }
    };

    // 跑帧：每 ~1.5 秒按一次 Start（推进过标题画面），
    // 记录早期/末帧哈希、末帧色彩数与音频能量
    let mut input = FrontendInput::default();
    let mut hash_early: Option<u64> = Option::None;
    let mut audio_sum = 0.0f64;
    let mut audio_count = 0usize;
    for frame in 0..frames {
        // 每 90 帧的前 3 帧按住 Start（开机即按 + 周期性再按）
        input
            .controller1
            .set_pressed(nes_sim::ControllerButton::Start, frame % 90 < 3);
        let snapshot = runtime.step(input);
        for &sample in snapshot.audio.samples {
            audio_sum += f64::from(sample.abs());
        }
        audio_count += snapshot.audio.samples.len();
        if frame == 5 {
            hash_early = Some(stable_byte_hash(snapshot.video.pixels));
        }
        outcome.frames = frame + 1;
    }
    let last_snapshot = runtime.snapshot();

    let pixels = last_snapshot.video.pixels;
    let colors = {
        let mut set = HashSet::new();
        let height = last_snapshot.video.height;
        let pitch = last_snapshot.video.pitch;
        set.extend(pixels[..height * pitch].iter().copied());
        set.len()
    };
    let hash_last = stable_byte_hash(pixels);
    outcome.distinct_colors = colors;
    outcome.audio_energy = if audio_count > 0 {
        audio_sum / audio_count as f64
    } else {
        0.0
    };
    // 冻结：画面非纯色，但从早期帧到末帧完全无变化（已含周期性 Start 输入）
    outcome.frozen = hash_early == Some(hash_last) && colors > BLANK_COLOR_THRESHOLD;
    let is_nsf = runtime.nes().nsf_info().is_some();
    outcome.status = if is_nsf {
        if outcome.audio_energy > AUDIO_SILENT_THRESHOLD {
            CompatStatus::SmokeOk
        } else {
            CompatStatus::Blank
        }
    } else if colors <= BLANK_COLOR_THRESHOLD {
        CompatStatus::Blank
    } else if outcome.frozen {
        CompatStatus::Frozen
    } else {
        CompatStatus::SmokeOk
    };
    match outcome.status {
        CompatStatus::Blank => outcome.detail = "画面几乎无输出".into(),
        CompatStatus::Frozen => outcome.detail = "画面冻结（全程无变化，含 Start 输入）".into(),
        _ => outcome.detail.clear(),
    }
    outcome.elapsed_ms = started.elapsed().as_millis() as u64;
    outcome
}
