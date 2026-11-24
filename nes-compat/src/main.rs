//! nes-compat：nes-sim 的 ROM 库浏览与兼容性测试台。
//!
//! 用法：
//!   nes-compat                                  # 图形界面
//!   nes-compat smoke --mapper 4 [--frames 300]  # 无窗口批量冒烟测试
//!   nes-compat smoke --game-id 532 [--limit 50]
//!   nes-compat stats                            # 打印按 mapper 的兼容性汇总

mod app;
mod archive;
mod compatdb;
mod config;
mod db;
mod player;
mod smoke;

use compatdb::{CompatStatus, ItemKind};
use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None | Some("gui") => run_gui(),
        Some("smoke") => run_smoke_cli(&args.collect::<Vec<_>>()),
        Some("stats") => run_stats_cli(),
        Some("help" | "--help" | "-h") => {
            eprintln!("用法: nes-compat [gui|smoke|stats|help]");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("未知子命令 {other:?}（可选 gui / smoke / stats）");
            ExitCode::from(2)
        }
    }
}

fn run_gui() -> ExitCode {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    match eframe::run_native(
        "nes-compat 兼容性测试台",
        native_options,
        Box::new(|cc| Ok(Box::new(app::CompatApp::new(cc)))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("GUI 启动失败: {error}");
            ExitCode::from(1)
        }
    }
}

fn run_smoke_cli(args: &[String]) -> ExitCode {
    let config = config::Config::load();
    let mut mapper: Option<i64> = Option::None;
    let mut game_id: Option<i64> = Option::None;
    let mut all = false;
    let mut frames = config.smoke_frames;
    let mut limit: i64 = 200;
    let mut untested_only = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--mapper" => {
                i += 1;
                mapper = args.get(i).and_then(|v| v.parse().ok());
                if mapper.is_none() {
                    eprintln!("--mapper 需要数字参数");
                    return ExitCode::from(2);
                }
            }
            "--game-id" => {
                i += 1;
                game_id = args.get(i).and_then(|v| v.parse().ok());
                if game_id.is_none() {
                    eprintln!("--game-id 需要数字参数");
                    return ExitCode::from(2);
                }
            }
            "--all" => all = true,
            "--frames" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse::<u32>().ok()) {
                    Some(value) if value >= 10 => frames = value,
                    _ => {
                        eprintln!("--frames 需要 ≥10 的数字");
                        return ExitCode::from(2);
                    }
                }
            }
            "--limit" => {
                i += 1;
                match args.get(i).and_then(|v| v.parse::<i64>().ok()) {
                    Some(value) if value > 0 => limit = value,
                    _ => {
                        eprintln!("--limit 需要 >0 的数字");
                        return ExitCode::from(2);
                    }
                }
            }
            "--untested-only" => untested_only = true,
            other => {
                eprintln!("未知参数 {other:?}");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }
    if !all && mapper.is_none() && game_id.is_none() {
        eprintln!("需要 --mapper N / --game-id ID / --all 之一");
        return ExitCode::from(2);
    }

    let Some(seven_zip) = config::find_7zz(&config) else {
        eprintln!("未找到 7-Zip（7zz/7z），请在 roms/compat.toml 配置 seven_zip");
        return ExitCode::from(1);
    };
    let db = match db::GameDb::open(&config.db_path) {
        Ok(db) => db,
        Err(error) => {
            eprintln!("打开 {} 失败: {error}", config.db_path.display());
            return ExitCode::from(1);
        }
    };
    let compat = match compatdb::open(&config.compat_db) {
        Ok(compat) => compat,
        Err(error) => {
            eprintln!("打开结果库失败: {error}");
            return ExitCode::from(1);
        }
    };
    let tested: std::collections::HashSet<String> = compatdb::load_all(&compat)
        .unwrap_or_default()
        .keys()
        .cloned()
        .collect();

    // 组装测试清单
    let mut items: Vec<smoke::SmokeItem> = Vec::new();
    let mut candidates: Vec<db::RomRow> = Vec::new();
    if let Some(game_id) = game_id {
        candidates = db.roms_for_game(game_id).unwrap_or_default();
    } else if all {
        // 全库抽样：按 mapper 覆盖度随机
        for mapper_info in db.mappers().unwrap_or_default() {
            if mapper_info.id == 0 {
                continue;
            }
            for (rom, _name) in db
                .roms_by_mapper(mapper_info.id, (limit * 4).min(20_000))
                .unwrap_or_default()
            {
                candidates.push(rom);
            }
        }
    } else if let Some(mapper) = mapper {
        for (rom, _name) in db
            .roms_by_mapper(mapper, (limit * 4).min(20_000))
            .unwrap_or_default()
        {
            candidates.push(rom);
        }
    }
    for rom in candidates {
        if rom.md5.is_empty() || rom.mapper.is_none() {
            continue; // 裸 dump 无法直接加载
        }
        if untested_only && tested.contains(&rom.md5) {
            continue;
        }
        items.push(smoke::SmokeItem {
            key: rom.md5.clone(),
            kind: ItemKind::Rom,
            game_id: rom.game_id,
            fname: rom.fname.clone(),
        });
        if items.len() as i64 >= limit && !all {
            break;
        }
    }
    if items.is_empty() {
        eprintln!("没有符合条件的 ROM（检查 --mapper / --untested-only / --limit）");
        return ExitCode::from(1);
    }
    eprintln!(
        "冒烟测试 {} 个 ROM × {} 帧（7z: {}, ever: {}）",
        items.len(),
        frames,
        seven_zip.display(),
        config.ever_dir.display()
    );

    let cache = Arc::new(archive::ArchiveCache::new(seven_zip));
    let cancel = AtomicBool::new(false);
    let progress = Mutex::new(smoke::SmokeProgress::default());
    let outcomes = smoke::run_smoke(
        items,
        &smoke::SmokeOptions {
            frames,
            cache,
            ever_dir: config.ever_dir.clone(),
            fds_bios: config.fds_bios_bytes(),
            compat_db: config.compat_db.clone(),
        },
        &cancel,
        Some(&progress),
        |outcome| {
            let progress = progress.lock().unwrap();
            eprintln!(
                "[{}/{}] {} → {} {} ({: >5}ms)",
                progress.done,
                progress.total,
                outcome.fname,
                outcome.status.as_str(),
                outcome.detail,
                outcome.elapsed_ms
            );
        },
    );
    // 汇总
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for outcome in &outcomes {
        *counts.entry(outcome.status.as_str()).or_default() += 1;
    }
    println!(
        "\n完成 {} 项（取消: {}）",
        outcomes.len(),
        cancel.load(Ordering::Relaxed)
    );
    for status in [
        CompatStatus::SmokeOk,
        CompatStatus::Blank,
        CompatStatus::Frozen,
        CompatStatus::LoadError,
        CompatStatus::Skip,
    ] {
        if let Some(count) = counts.get(status.as_str()) {
            println!("  {:<10} {}", status.as_str(), count);
        }
    }
    println!("结果已写入 {}", config.compat_db.display());
    ExitCode::SUCCESS
}

fn run_stats_cli() -> ExitCode {
    let config = config::Config::load();
    let db = match db::GameDb::open(&config.db_path) {
        Ok(db) => db,
        Err(error) => {
            eprintln!("打开 {} 失败: {error}", config.db_path.display());
            return ExitCode::from(1);
        }
    };
    let compat = match compatdb::open(&config.compat_db) {
        Ok(compat) => compat,
        Err(error) => {
            eprintln!("打开结果库失败: {error}");
            return ExitCode::from(1);
        }
    };
    let results = compatdb::load_all(&compat).unwrap_or_default();
    let mappers = db.mappers().unwrap_or_default();
    let index = db.rom_index().unwrap_or_default();

    // per-mapper 聚合（含裸 dump NULL 桶）
    #[derive(Default)]
    struct Agg {
        total: usize,
        tested: usize,
        ok: usize,
        minor: usize,
        major: usize,
        broken: usize,
        auto_bad: usize,
    }
    let mut aggs: HashMap<i64, Agg> = HashMap::new();
    let mut bare = Agg::default();
    for (game_id, mapper, md5) in &index {
        let _ = game_id;
        let agg = match mapper {
            Some(mapper) => aggs.entry(*mapper).or_default(),
            Option::None => &mut bare,
        };
        agg.total += 1;
        if let Some(row) = results.get(md5) {
            agg.tested += 1;
            match row.status {
                CompatStatus::Ok => agg.ok += 1,
                CompatStatus::Minor => agg.minor += 1,
                CompatStatus::Major => agg.major += 1,
                CompatStatus::Broken => agg.broken += 1,
                CompatStatus::Blank | CompatStatus::Frozen | CompatStatus::LoadError => {
                    agg.auto_bad += 1
                }
                _ => {}
            }
        }
    }
    println!(
        "{:>4} {:<14} {:>4} {:>7} {:>7} {:>4} {:>5} {:>5} {:>6} {:>8}",
        "map", "名称", "支持", "ROM数", "已测", "OK", "小问题", "严重", "无法运行", "自动异常"
    );
    let name_of = |id: i64| -> String {
        mappers
            .iter()
            .find(|m| m.id == id)
            .map(|m| m.name.clone())
            .unwrap_or_default()
    };
    let mut rows: Vec<(i64, Agg)> = aggs.into_iter().collect();
    rows.sort_by_key(|(id, _)| *id);
    for (id, agg) in rows {
        println!(
            "{:>4} {:<14} {:>4} {:>7} {:>7} {:>4} {:>5} {:>5} {:>6} {:>8}",
            id,
            name_of(id),
            if mappers.iter().any(|m| m.id == id && m.supported) {
                "✓"
            } else {
                ""
            },
            agg.total,
            agg.tested,
            agg.ok,
            agg.minor,
            agg.major,
            agg.broken,
            agg.auto_bad
        );
    }
    println!(
        "{:>4} {:<14} {:>4} {:>7} {:>7} {:>4} {:>5} {:>5} {:>6} {:>8}",
        "NULL",
        "裸dump(无头)",
        "",
        bare.total,
        bare.tested,
        bare.ok,
        bare.minor,
        bare.major,
        bare.broken,
        bare.auto_bad
    );
    ExitCode::SUCCESS
}
