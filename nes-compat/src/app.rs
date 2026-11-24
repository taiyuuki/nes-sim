//! egui 界面：游戏库 / 详情 / NSF 库 / 冒烟测试 / 统计 / 设置 / 播放视图。

use crate::archive::ArchiveCache;
use crate::compatdb::{self, CompatStatus, ItemKind, ResultRow};
use crate::config::{self, Config};
use crate::db::{self, GameDb, GameListItem, MapperInfo, NsfRow, RomRow};
use crate::player::{PlayRequest, PlayerHandle};
use crate::smoke::{self, SmokeItem, SmokeOutcome, SmokeProgress};
use egui::{Color32, ColorImage, TextureHandle, TextureOptions, Ui, Vec2};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq)]
enum View {
    Library,
    Detail(i64),
    Nsf,
    Smoke,
    Stats,
    Settings,
    Play,
}

#[derive(Debug, Clone)]
struct PlaySession {
    key: String,
    kind: ItemKind,
    fname: String,
    game_id: i64,
    display: String,
}

#[derive(Default)]
struct StatsRow {
    mapper: Option<i64>,
    name: String,
    supported: bool,
    total: usize,
    tested: usize,
    ok: usize,
    minor: usize,
    major: usize,
    broken: usize,
    auto_bad: usize,
}

pub struct CompatApp {
    ctx: egui::Context,
    config: Config,
    db: Option<GameDb>,
    db_error: String,
    compat: Option<Connection>,
    cache: Arc<ArchiveCache>,
    archive_error: String,

    mappers: Vec<MapperInfo>,
    types: Vec<String>,
    filter_text: String,
    filter_mapper: Option<i64>,
    filter_type: Option<String>,
    untested_only: bool,
    games: Vec<GameListItem>,
    results: HashMap<String, ResultRow>,
    /// game_id -> (ROM 总数, 已测数)
    game_progress: HashMap<i64, (usize, usize)>,
    stats_rows: Vec<StatsRow>,
    stats_dirty: bool,

    view: View,
    return_view: View,
    detail_roms: Vec<RomRow>,
    detail_nsfs: Vec<NsfRow>,
    detail_title: String,
    detail_meta: String,

    player: PlayerHandle,
    play: Option<PlaySession>,
    note_buf: String,
    auto_advance: bool,
    ui_message: String,

    nsf_text: String,
    nsf_results: Vec<NsfRow>,

    smoke_mapper: Option<i64>,
    smoke_frames: u32,
    smoke_limit: u32,
    smoke_untested_only: bool,
    smoke_cancel: Arc<AtomicBool>,
    smoke_progress: Arc<Mutex<SmokeProgress>>,
    smoke_outcomes: Arc<Mutex<Vec<SmokeOutcome>>>,
    smoke_active: bool,
    smoke_summary_loaded: bool,

    tex: TextureHandle,
    last_frame_no: u64,
    /// 屏幕虚拟手柄按住状态：A B Select Start ↑ ↓ ← →
    virtual_pad: (bool, bool, bool, bool, bool, bool, bool, bool),
}

impl CompatApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        let config = Config::load();
        install_cjk_font(&cc.egui_ctx, &config);

        let (db, db_error) = match GameDb::open(&config.db_path) {
            Ok(db) => (Some(db), String::new()),
            Err(error) => (
                Option::None,
                format!("打开 {} 失败: {error}", config.db_path.display()),
            ),
        };
        let (compat, compat_error) = match compatdb::open(&config.compat_db) {
            Ok(conn) => (Some(conn), String::new()),
            Err(error) => (Option::None, format!("打开结果库失败: {error}")),
        };
        let results = compat
            .as_ref()
            .and_then(|conn| compatdb::load_all(conn).ok())
            .unwrap_or_default();
        let (cache, archive_error) = match config::find_7zz(&config) {
            Some(seven_zip) => (Arc::new(ArchiveCache::new(seven_zip)), String::new()),
            Option::None => (
                Arc::new(ArchiveCache::new(PathBuf::from("7zz"))),
                "未找到 7-Zip，无法解压 ROM 包（可在设置中指定路径）".into(),
            ),
        };
        let mappers = db
            .as_ref()
            .and_then(|db| db.mappers().ok())
            .unwrap_or_default();
        let types = db
            .as_ref()
            .and_then(|db| db.types().ok())
            .unwrap_or_default();
        let smoke_frames = config.smoke_frames;

        let tex = cc.egui_ctx.load_texture(
            "play-frame",
            ColorImage::from_rgba_unmultiplied(
                [nes_sim::FRAME_WIDTH, nes_sim::FRAME_HEIGHT],
                &vec![0u8; nes_sim::FRAME_WIDTH * nes_sim::FRAME_HEIGHT * 4],
            ),
            TextureOptions::NEAREST,
        );

        let mut app = Self {
            ctx: cc.egui_ctx.clone(),
            db,
            db_error: if db_error.is_empty() {
                compat_error
            } else {
                db_error
            },
            compat,
            cache,
            archive_error,
            mappers,
            types,
            filter_text: String::new(),
            filter_mapper: Option::None,
            filter_type: Option::None,
            untested_only: false,
            games: Vec::new(),
            results,
            game_progress: HashMap::new(),
            stats_rows: Vec::new(),
            stats_dirty: true,
            view: View::Library,
            return_view: View::Library,
            detail_roms: Vec::new(),
            detail_nsfs: Vec::new(),
            detail_title: String::new(),
            detail_meta: String::new(),
            player: PlayerHandle::spawn(cc.egui_ctx.clone()),
            play: Option::None,
            note_buf: String::new(),
            auto_advance: true,
            ui_message: String::new(),
            nsf_text: String::new(),
            nsf_results: Vec::new(),
            smoke_mapper: Option::None,
            smoke_frames,
            smoke_limit: 200,
            smoke_untested_only: true,
            smoke_cancel: Arc::new(AtomicBool::new(false)),
            smoke_progress: Arc::new(Mutex::new(SmokeProgress::default())),
            smoke_outcomes: Arc::new(Mutex::new(Vec::new())),
            smoke_active: false,
            smoke_summary_loaded: true,
            tex,
            last_frame_no: u64::MAX,
            virtual_pad: Default::default(),
            config,
        };
        app.refresh_games();
        app
    }

    // ---------- 数据刷新 ----------

    fn refresh_games(&mut self) {
        let Some(db) = &self.db else { return };
        let filter = db::GameFilter {
            text: self.filter_text.clone(),
            mapper: self.filter_mapper,
            type1: self.filter_type.clone(),
        };
        match db.search_games(&filter) {
            Ok(games) => self.games = games,
            Err(error) => self.ui_message = format!("检索失败: {error}"),
        }
        if self.untested_only {
            self.games.retain(|game| {
                let (total, tested) = self.game_progress.get(&game.id).copied().unwrap_or((0, 0));
                total > 0 && tested < total
            });
        }
    }

    /// 重建内存统计（结果库变化后调用）。
    fn refresh_stats(&mut self) {
        self.stats_dirty = false;
        self.game_progress.clear();
        let Some(db) = &self.db else { return };
        let Ok(index) = db.rom_index() else { return };
        for (game_id, _mapper, md5) in &index {
            let entry = self.game_progress.entry(*game_id).or_insert((0, 0));
            entry.0 += 1;
            if self.results.contains_key(md5) {
                entry.1 += 1;
            }
        }
        // mapper 聚合
        let mut aggs: HashMap<Option<i64>, StatsRow> = HashMap::new();
        for mapper in &self.mappers {
            aggs.insert(
                Some(mapper.id),
                StatsRow {
                    mapper: Some(mapper.id),
                    name: mapper.name.clone(),
                    supported: mapper.supported,
                    ..Default::default()
                },
            );
        }
        for (_game_id, mapper, md5) in &index {
            let row = aggs.entry(*mapper).or_insert_with(|| StatsRow {
                mapper: *mapper,
                ..Default::default()
            });
            row.total += 1;
            if let Some(result) = self.results.get(md5) {
                row.tested += 1;
                match result.status {
                    CompatStatus::Ok => row.ok += 1,
                    CompatStatus::Minor => row.minor += 1,
                    CompatStatus::Major => row.major += 1,
                    CompatStatus::Broken => row.broken += 1,
                    CompatStatus::Blank | CompatStatus::Frozen | CompatStatus::LoadError => {
                        row.auto_bad += 1
                    }
                    _ => {}
                }
            }
        }
        let mut rows: Vec<StatsRow> = aggs.into_values().collect();
        rows.sort_by_key(|row| row.mapper);
        self.stats_rows = rows;
    }

    fn open_detail(&mut self, game_id: i64) {
        let Some(db) = &self.db else { return };
        self.detail_roms = db.roms_for_game(game_id).unwrap_or_default();
        self.detail_nsfs = db.nsfs_for_game(game_id).unwrap_or_default();
        let detail = db.game_detail(game_id).ok().flatten();
        let title = detail
            .as_ref()
            .map(|detail| {
                let item = &detail.item;
                let mut name = if item.name_cn.is_empty() {
                    item.name_en.clone()
                } else {
                    item.name_cn.clone()
                };
                if !item.name_en.is_empty() && item.name_en != name {
                    name.push_str(" / ");
                    name.push_str(&item.name_en);
                }
                name
            })
            .unwrap_or_else(|| format!("游戏 #{game_id}"));
        self.detail_meta = detail
            .map(|detail| {
                let item = detail.item;
                let mut parts = Vec::new();
                let mut kind = item.type1.clone();
                if !detail.type2.is_empty() {
                    kind.push('/');
                    kind.push_str(&detail.type2);
                }
                if !kind.is_empty() {
                    parts.push(kind);
                }
                if !detail.release_date.is_empty() {
                    parts.push(format!("发售 {}", detail.release_date));
                } else if item.release_year > 0 {
                    parts.push(format!("{} 年", item.release_year));
                }
                if !item.company.is_empty() {
                    parts.push(item.company);
                }
                if item.playernum > 0 {
                    parts.push(format!("{} 人", item.playernum));
                }
                if item.good > 0 {
                    parts.push(format!(
                        "推荐度 {}",
                        "★".repeat(item.good.clamp(0, 5) as usize)
                    ));
                }
                parts.join(" · ")
            })
            .unwrap_or_default();
        self.detail_title = title;
        self.view = View::Detail(game_id);
    }

    // ---------- 运行与标记 ----------

    fn launch_rom(&mut self, rom: &RomRow, game_name: &str) {
        let session = PlaySession {
            key: rom.md5.clone(),
            kind: ItemKind::Rom,
            fname: rom.fname.clone(),
            game_id: rom.game_id,
            display: format!("{} · {}", game_name.trim(), rom.fname),
        };
        self.spawn_extract_and_play(session, rom.game_id, rom.fname.clone(), false);
    }

    fn launch_nsf(&mut self, nsf: &NsfRow) {
        let session = PlaySession {
            key: nsf.md5.clone(),
            kind: ItemKind::Nsf,
            fname: nsf.fname.clone(),
            game_id: nsf.game_id,
            display: format!("♪ {} · {}", nsf.title, nsf.artist),
        };
        self.spawn_extract_and_play(session, nsf.game_id, nsf.fname.clone(), true);
    }

    fn spawn_extract_and_play(
        &mut self,
        session: PlaySession,
        game_id: i64,
        fname: String,
        nsf: bool,
    ) {
        if !self.archive_error.is_empty() {
            self.ui_message = self.archive_error.clone();
            return;
        }
        let player = self.player.clone();
        let ctx = self.ctx.clone();
        let cache = Arc::clone(&self.cache);
        let ever_dir = self.config.ever_dir.clone();
        let bios = self.config.fds_bios_bytes();
        self.ui_message = format!("解压 {fname}…");
        self.return_view = if self.view == View::Play {
            self.return_view.clone()
        } else {
            self.view.clone()
        };
        self.view = View::Play;
        let request_session = session.clone();
        self.note_buf = self
            .results
            .get(&session.key)
            .map(|row| row.note.clone())
            .unwrap_or_default();
        self.play = Some(session);
        std::thread::spawn(move || {
            match cache.extract_by_name(&ever_dir, game_id, &fname, nsf) {
                Ok(bytes) => {
                    player.load(PlayRequest {
                        bytes,
                        fname: request_session.fname.clone(),
                        display: request_session.display.clone(),
                        fds_bios: bios,
                    });
                }
                Err(error) => {
                    player.stop();
                    player.set_message(format!("解压失败: {error}"));
                }
            }
            ctx.request_repaint();
        });
    }

    fn mark_current(&mut self, status: CompatStatus) {
        let Some(session) = self.play.clone() else {
            return;
        };
        if let Some(conn) = &self.compat
            && let Err(error) = compatdb::set_result(
                conn,
                &session.key,
                session.kind,
                &session.fname,
                session.game_id,
                status,
                &self.note_buf,
            )
        {
            self.ui_message = format!("写入结果失败: {error}");
            return;
        }
        self.results.insert(
            session.key.clone(),
            ResultRow {
                key: session.key.clone(),
                kind: session.kind,
                fname: session.fname.clone(),
                game_id: session.game_id,
                status,
                note: self.note_buf.clone(),
                manual: true,
                updated_at: String::new(),
            },
        );
        self.stats_dirty = true;
        self.ui_message = format!("已标记: {} {}", session.fname, status.label());
        if self.auto_advance {
            self.next_untested();
        }
    }

    fn next_untested(&mut self) {
        let Some(db) = &self.db else { return };
        let tested: HashSet<String> = self.results.keys().cloned().collect();
        match db.random_untested_rom(&tested, self.filter_mapper) {
            Ok(Some((rom, game_name))) => self.launch_rom(&rom, &game_name),
            Ok(Option::None) => {
                self.ui_message = "没有更多未测试的 ROM".into();
            }
            Err(error) => self.ui_message = format!("查询失败: {error}"),
        }
    }

    // ---------- 冒烟测试 ----------

    fn smoke_items_for_mapper(&mut self, mapper: Option<i64>) -> Vec<SmokeItem> {
        let Some(db) = &self.db else {
            return Vec::new();
        };
        let mut items = Vec::new();
        if let Some(mapper) = mapper {
            let over_fetch = self.smoke_limit.saturating_mul(4).min(20_000) as i64;
            for (rom, _name) in db.roms_by_mapper(mapper, over_fetch).unwrap_or_default() {
                if rom.mapper.is_none() || rom.md5.is_empty() {
                    continue;
                }
                if self.smoke_untested_only && self.results.contains_key(&rom.md5) {
                    continue;
                }
                items.push(SmokeItem {
                    key: rom.md5.clone(),
                    kind: ItemKind::Rom,
                    game_id: rom.game_id,
                    fname: rom.fname.clone(),
                });
                if items.len() >= self.smoke_limit as usize {
                    break;
                }
            }
        }
        items
    }

    fn start_smoke(&mut self, items: Vec<SmokeItem>) {
        if self.smoke_active {
            self.ui_message = "冒烟测试已在运行".into();
            return;
        }
        if !self.archive_error.is_empty() {
            self.ui_message = self.archive_error.clone();
            return;
        }
        if items.is_empty() {
            self.ui_message = "没有符合条件的 ROM".into();
            return;
        }
        self.smoke_cancel = Arc::new(AtomicBool::new(false));
        *self.smoke_outcomes.lock().unwrap() = Vec::new();
        {
            let mut progress = self.smoke_progress.lock().unwrap();
            *progress = SmokeProgress {
                total: items.len(),
                ..Default::default()
            };
        }
        let cache = Arc::clone(&self.cache);
        let ever_dir = self.config.ever_dir.clone();
        let bios = self.config.fds_bios_bytes();
        let compat_db = self.config.compat_db.clone();
        let cancel = Arc::clone(&self.smoke_cancel);
        let progress = Arc::clone(&self.smoke_progress);
        let outcomes = Arc::clone(&self.smoke_outcomes);
        let ctx = self.ctx.clone();
        let frames = self.smoke_frames;
        self.smoke_active = true;
        self.smoke_summary_loaded = false;
        std::thread::spawn(move || {
            smoke::run_smoke(
                items,
                &smoke::SmokeOptions {
                    frames,
                    cache,
                    ever_dir,
                    fds_bios: bios,
                    compat_db,
                },
                &cancel,
                Some(&progress),
                |outcome| {
                    outcomes.lock().unwrap().push(outcome.clone());
                    ctx.request_repaint();
                },
            );
            ctx.request_repaint();
        });
    }

    fn poll_smoke(&mut self) {
        if !self.smoke_active {
            return;
        }
        let finished = self.smoke_progress.lock().unwrap().finished;
        if finished {
            self.smoke_active = false;
        }
        if !self.smoke_active && !self.smoke_summary_loaded {
            self.smoke_summary_loaded = true;
            // 结果库已被冒烟线程写入，重载内存缓存
            if let Some(conn) = &self.compat
                && let Ok(results) = compatdb::load_all(conn)
            {
                self.results = results;
                self.stats_dirty = true;
            }
        }
    }

    // ---------- 输入 ----------

    fn collect_play_input(&mut self, ctx: &egui::Context) {
        use egui::Key;
        // 有文本输入焦点时（备注框），不把键盘当手柄
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let down = |key: Key| ctx.input(|i| i.key_down(key));
        let pressed = |key: Key| ctx.input(|i| i.key_pressed(key));
        let fast_forward = down(Key::Space);
        self.player.set_fast_forward(fast_forward);
        let pause = pressed(Key::P);
        let reset = pressed(Key::R);
        let esc = pressed(Key::Escape);
        let fds_toggle = pressed(Key::F1);
        let fds_side = pressed(Key::F2);
        let nsf_prev = pressed(Key::OpenBracket);
        let nsf_next = pressed(Key::CloseBracket);
        let (vp1_a, vp1_b, vp1_select, vp1_start, vp1_up, vp1_down, vp1_left, vp1_right) =
            self.virtual_pad;
        self.player.with_input(|input| {
            use nes_sim::ControllerButton as Cb;
            let set = |state: &mut nes_sim::ControllerState, on: bool, button: Cb| {
                state.set_pressed(button, on);
            };
            set(&mut input.p1, down(Key::X) || vp1_a, Cb::A);
            set(&mut input.p1, down(Key::Z) || vp1_b, Cb::B);
            set(&mut input.p1, down(Key::Tab) || vp1_select, Cb::Select);
            set(&mut input.p1, down(Key::Enter) || vp1_start, Cb::Start);
            set(&mut input.p1, down(Key::ArrowUp) || vp1_up, Cb::Up);
            set(&mut input.p1, down(Key::ArrowDown) || vp1_down, Cb::Down);
            set(&mut input.p1, down(Key::ArrowLeft) || vp1_left, Cb::Left);
            set(&mut input.p1, down(Key::ArrowRight) || vp1_right, Cb::Right);
            if pause {
                input.toggle_pause = true;
            }
            if reset {
                input.reset = true;
            }
            if fds_toggle {
                input.fds_toggle_insert = true;
            }
            if fds_side {
                input.fds_next_side = true;
            }
            if nsf_prev {
                input.nsf_track_delta = -1;
            }
            if nsf_next {
                input.nsf_track_delta = 1;
            }
        });
        if esc && self.view == View::Play {
            self.exit_play();
        }
    }

    fn exit_play(&mut self) {
        self.player.stop();
        self.player.set_fast_forward(false);
        self.play = Option::None;
        self.view = self.return_view.clone();
    }

    fn update_play_texture(&mut self) {
        let frame = self.player.snapshot_frame();
        if frame.frame_no != self.last_frame_no {
            self.last_frame_no = frame.frame_no;
            self.tex.set(
                ColorImage::from_rgba_unmultiplied([frame.width, frame.height], &frame.rgba),
                TextureOptions::NEAREST,
            );
        }
    }
}

impl eframe::App for CompatApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.poll_smoke();
        if self.stats_dirty && self.view == View::Stats {
            self.refresh_stats();
        }

        // 顶部标签栏
        egui::Panel::top("tabs").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let tab = |ui: &mut Ui, view: &mut View, target: View, label: &str| {
                    if ui.selectable_label(*view == target, label).clicked() {
                        *view = target;
                    }
                };
                tab(ui, &mut self.view, View::Library, "📚 游戏库");
                tab(ui, &mut self.view, View::Nsf, "🎵 NSF 库");
                tab(ui, &mut self.view, View::Smoke, "🔥 冒烟测试");
                tab(ui, &mut self.view, View::Stats, "📊 统计");
                tab(ui, &mut self.view, View::Settings, "⚙️ 设置");
                ui.separator();
                if self.player.is_running() && ui.button("▶ 播放中…").clicked() {
                    self.view = View::Play;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if !self.db_error.is_empty() {
                        ui.colored_label(Color32::RED, &self.db_error);
                    } else if !self.ui_message.is_empty() {
                        ui.label(&self.ui_message);
                    }
                });
            });
            ui.add_space(2.0);
        });

        if self.view == View::Play && (self.player.is_running() || self.play.is_some()) {
            self.collect_play_input(ui.ctx());
            self.update_play_texture();
            self.play_view(ui);
            return;
        }

        match self.view.clone() {
            View::Library => self.library_view(ui),
            View::Detail(game_id) => {
                self.back_bar(ui, View::Library);
                self.detail_view(ui, game_id);
            }
            View::Nsf => self.nsf_view(ui),
            View::Smoke => self.smoke_view(ui),
            View::Stats => self.stats_view(ui),
            View::Settings => self.settings_view(ui),
            View::Play => {
                // 播放已停止但视图还停在 Play
                self.view = self.return_view.clone();
            }
        }
    }
}

impl CompatApp {
    /// 顶部返回条（详情视图用）。
    fn back_bar(&mut self, ui: &mut Ui, target: View) {
        egui::Panel::top("back").show(ui, |ui| {
            if ui.button("← 返回").clicked() {
                self.view = target;
            }
        });
    }

    // ---------- 游戏库 ----------

    fn library_view(&mut self, ui: &mut Ui) {
        egui::Panel::left("filters")
            .resizable(true)
            .default_size(300.0)
            .show(ui, |ui| {
                ui.add_space(6.0);
                ui.heading("筛选");
                ui.add_space(4.0);
                let mut search = false;
                ui.horizontal(|ui| {
                    let edit = egui::TextEdit::singleline(&mut self.filter_text)
                        .hint_text("游戏名（中/英/日）")
                        .desired_width(ui.available_width() - 50.0);
                    let response = ui.add(edit);
                    search |=
                        response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button("🔍").clicked() {
                        search = true;
                    }
                });
                ui.add_space(6.0);
                let mapper_label = self
                    .filter_mapper
                    .and_then(|id| self.mappers.iter().find(|m| m.id == id))
                    .map(|m| format!("{} {}", m.id, m.name))
                    .unwrap_or_else(|| "全部".into());
                egui::ComboBox::from_id_salt("mapper")
                    .selected_text(format!("Mapper: {mapper_label}"))
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.filter_mapper, Option::None, "全部");
                        for mapper in self.mappers.clone() {
                            if mapper.rom_count == 0 {
                                continue;
                            }
                            ui.selectable_value(
                                &mut self.filter_mapper,
                                Some(mapper.id),
                                format!("{} {}（{}）", mapper.id, mapper.name, mapper.rom_count),
                            );
                        }
                    });
                let type_label = self.filter_type.clone().unwrap_or_else(|| "全部".into());
                egui::ComboBox::from_id_salt("type1")
                    .selected_text(format!("类型: {type_label}"))
                    .width(ui.available_width())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.filter_type, Option::None, "全部");
                        for type_name in self.types.clone() {
                            ui.selectable_value(
                                &mut self.filter_type,
                                Some(type_name.clone()),
                                type_name,
                            );
                        }
                    });
                if ui
                    .checkbox(&mut self.untested_only, "仅显示有未测 ROM 的游戏")
                    .changed()
                {
                    search = true;
                }
                if search {
                    self.refresh_games();
                }
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(4.0);
                // 库进度（首次进入时惰性计算）
                if self.game_progress.is_empty() {
                    self.refresh_stats();
                }
                let total: usize = self.game_progress.values().map(|(t, _)| t).sum();
                let tested: usize = self.game_progress.values().map(|(_, d)| d).sum();
                ui.label(format!("ROM 测试进度：{tested} / {total}"));
                let fraction = if total > 0 {
                    tested as f32 / total as f32
                } else {
                    0.0
                };
                ui.add(egui::ProgressBar::new(fraction).show_percentage());
                ui.add_space(8.0);
                if ui.button("🎲 下一个未测试").clicked() {
                    self.next_untested();
                }
            });

        egui::CentralPanel::default().show(ui, |ui| {
            let row_height = ui.text_style_height(&egui::TextStyle::Body) + 8.0;
            let mut open: Option<i64> = Option::None;
            let games = &self.games;
            let progress = &self.game_progress;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(ui, row_height, games.len(), |ui, range| {
                    for game in &games[range] {
                        let (total, tested) = progress.get(&game.id).copied().unwrap_or((0, 0));
                        let badge = if total > 0 && tested == total {
                            "✅"
                        } else if tested > 0 {
                            "◐"
                        } else {
                            "·"
                        };
                        ui.horizontal(|ui| {
                            if ui
                                .selectable_label(
                                    false,
                                    egui::RichText::new(format!("{badge} {}", display_name(game)))
                                        .strong(),
                                )
                                .clicked()
                            {
                                open = Some(game.id);
                            }
                            ui.add_space(8.0);
                            ui.weak(format!(
                                "{} · {} · {}ROM",
                                if game.company.is_empty() {
                                    "—"
                                } else {
                                    &game.company
                                },
                                if game.release_year > 0 {
                                    game.release_year.to_string()
                                } else {
                                    "—".to_string()
                                },
                                game.romnum
                            ));
                        });
                        ui.add_space(2.0);
                    }
                });
            if let Some(id) = open {
                self.open_detail(id);
            }
        });
    }

    fn detail_view(&mut self, ui: &mut Ui, game_id: i64) {
        let _ = game_id; // 明细已在 open_detail 载入
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(&self.detail_title);
            if !self.detail_meta.is_empty() {
                ui.weak(&self.detail_meta);
            }
            ui.add_space(6.0);
            let mut run_smoke_all = false;
            let mut launch: Option<RomRow> = Option::None;
            let mut launch_nsf: Option<NsfRow> = Option::None;
            let title = self.detail_title.clone();
            let detail_roms = &self.detail_roms;
            let detail_nsfs = &self.detail_nsfs;
            let results = &self.results;
            ui.horizontal(|ui| {
                if ui.button("🔥 本游戏全部冒烟").clicked() {
                    run_smoke_all = true;
                }
                if ui.button("▶ 运行随机版本").clicked()
                    && let Some(rom) = pick_random(detail_roms)
                {
                    launch = Some(rom.clone());
                }
            });
            ui.add_space(6.0);
            ui.separator();

            ui.strong(format!("ROM（{}）", detail_roms.len()));
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("roms")
                        .striped(true)
                        .num_columns(6)
                        .min_col_width(60.0)
                        .show(ui, |ui| {
                            ui.strong("文件");
                            ui.strong("Mapper");
                            ui.strong("容量");
                            ui.strong("大小");
                            ui.strong("结果");
                            ui.strong("操作");
                            ui.end_row();
                            for rom in detail_roms {
                                ui.label(&rom.fname);
                                ui.label(mapper_desc(rom));
                                ui.label(format!("{}K/{}K", rom.prg_kb, rom.chr_kb));
                                ui.label(human_size(rom.size));
                                let status = results
                                    .get(&rom.md5)
                                    .map(|row| {
                                        egui::RichText::new(row.status.label()).color(
                                            if row.status.is_manual() {
                                                Color32::LIGHT_GREEN
                                            } else {
                                                Color32::GRAY
                                            },
                                        )
                                    })
                                    .unwrap_or_else(|| egui::RichText::new("未测").weak());
                                ui.label(status);
                                if ui.small_button("▶ 运行").clicked() {
                                    launch = Some(rom.clone());
                                }
                                ui.end_row();
                            }
                        });
                    if !detail_nsfs.is_empty() {
                        ui.add_space(10.0);
                        ui.strong(format!("NSF 音乐（{}）", detail_nsfs.len()));
                        egui::Grid::new("nsfs")
                            .striped(true)
                            .num_columns(5)
                            .show(ui, |ui| {
                                ui.strong("曲名");
                                ui.strong("艺术家");
                                ui.strong("音源");
                                ui.strong("曲目");
                                ui.strong("操作");
                                ui.end_row();
                                for nsf in detail_nsfs {
                                    ui.label(&nsf.title);
                                    ui.label(&nsf.artist);
                                    ui.label(&nsf.chips);
                                    ui.label(format!("{}曲 · {}", nsf.songs, nsf.timing));
                                    if ui.small_button("▶").clicked() {
                                        launch_nsf = Some(nsf.clone());
                                    }
                                    ui.end_row();
                                }
                            });
                    }
                });
            if run_smoke_all {
                let items: Vec<SmokeItem> = detail_roms
                    .iter()
                    .filter(|rom| rom.mapper.is_some() && !rom.md5.is_empty())
                    .map(|rom| SmokeItem {
                        key: rom.md5.clone(),
                        kind: ItemKind::Rom,
                        game_id: rom.game_id,
                        fname: rom.fname.clone(),
                    })
                    .collect();
                self.view = View::Smoke;
                self.start_smoke(items);
            } else if let Some(rom) = launch {
                self.launch_rom(&rom, &title);
            } else if let Some(nsf) = launch_nsf {
                self.launch_nsf(&nsf);
            }
        });
    }

    fn nsf_view(&mut self, ui: &mut Ui) {
        egui::Panel::top("nsf-search").show(ui, |ui| {
            ui.horizontal(|ui| {
                let mut search = false;
                let edit = egui::TextEdit::singleline(&mut self.nsf_text)
                    .hint_text("曲名 / 艺术家 / 游戏名")
                    .desired_width(320.0);
                let response = ui.add(edit);
                search |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("🔍 搜索").clicked() {
                    search = true;
                }
                if search && let Some(db) = &self.db {
                    match db.search_nsfs(&self.nsf_text, 2000) {
                        Ok(rows) => self.nsf_results = rows,
                        Err(error) => self.ui_message = format!("NSF 检索失败: {error}"),
                    }
                }
            });
        });
        egui::CentralPanel::default().show(ui, |ui| {
            let row_height = ui.text_style_height(&egui::TextStyle::Body) + 8.0;
            let mut launch: Option<NsfRow> = Option::None;
            let results = &self.nsf_results;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(ui, row_height, results.len(), |ui, range| {
                    for nsf in &results[range] {
                        let response = ui
                            .horizontal(|ui| {
                                ui.label(format!("♪ {}", nsf.title));
                                ui.weak(format!(
                                    "{} · {} · {}曲",
                                    nsf.artist, nsf.chips, nsf.songs
                                ));
                                ui.weak(&nsf.game_name);
                            })
                            .response;
                        if response.clicked() {
                            launch = Some(nsf.clone());
                        }
                        ui.add_space(2.0);
                    }
                });
            if let Some(nsf) = launch {
                self.launch_nsf(&nsf);
            }
        });
    }

    fn smoke_view(&mut self, ui: &mut Ui) {
        egui::Panel::top("smoke-config").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("范围 Mapper:");
                let label = self
                    .smoke_mapper
                    .and_then(|id| self.mappers.iter().find(|m| m.id == id))
                    .map(|m| format!("{} {}", m.id, m.name))
                    .unwrap_or_else(|| "选择…".into());
                egui::ComboBox::from_id_salt("smoke-mapper")
                    .selected_text(label)
                    .width(160.0)
                    .show_ui(ui, |ui| {
                        for mapper in self.mappers.clone() {
                            if mapper.rom_count == 0 {
                                continue;
                            }
                            ui.selectable_value(
                                &mut self.smoke_mapper,
                                Some(mapper.id),
                                format!("{} {}（{}）", mapper.id, mapper.name, mapper.rom_count),
                            );
                        }
                    });
                ui.label("帧数:");
                ui.add(
                    egui::DragValue::new(&mut self.smoke_frames)
                        .range(30..=10000)
                        .speed(10),
                );
                ui.label("数量:");
                ui.add(
                    egui::DragValue::new(&mut self.smoke_limit)
                        .range(1..=5000)
                        .speed(10),
                );
                ui.checkbox(&mut self.smoke_untested_only, "仅未测试");
                if self.smoke_active {
                    if ui.button("⏹ 停止").clicked() {
                        self.smoke_cancel.store(true, Ordering::Relaxed);
                    }
                } else if ui.button("🔥 开始").clicked() {
                    let items = self.smoke_items_for_mapper(self.smoke_mapper);
                    self.start_smoke(items);
                }
            });
            let progress = self.smoke_progress.lock().unwrap().clone();
            if progress.total > 0 {
                let fraction = progress.done as f32 / progress.total as f32;
                let eta = if progress.done > 0
                    && let Some(started) = progress.started_at
                {
                    let elapsed = started.elapsed().as_secs_f32();
                    let remaining =
                        elapsed / progress.done as f32 * (progress.total - progress.done) as f32;
                    format!("剩余 {:.0} 分钟", remaining / 60.0)
                } else {
                    String::new()
                };
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .text(format!(
                            "{}/{} {} {}",
                            progress.done,
                            progress.total,
                            eta,
                            if progress.finished {
                                "（已完成）"
                            } else {
                                ""
                            }
                        ))
                        .show_percentage(),
                );
                if !progress.current.is_empty() {
                    ui.label(format!("当前: {}", progress.current));
                }
            }
        });
        egui::CentralPanel::default().show(ui, |ui| {
            let outcomes = self.smoke_outcomes.lock().unwrap().clone();
            if outcomes.is_empty() {
                ui.weak("尚无结果。选择 mapper 后点「开始」。");
                return;
            }
            let mut counts: HashMap<CompatStatus, usize> = HashMap::new();
            for outcome in &outcomes {
                *counts.entry(outcome.status).or_default() += 1;
            }
            ui.horizontal(|ui| {
                for status in [
                    CompatStatus::SmokeOk,
                    CompatStatus::Blank,
                    CompatStatus::Frozen,
                    CompatStatus::LoadError,
                    CompatStatus::Skip,
                ] {
                    if let Some(count) = counts.get(&status) {
                        ui.label(format!("{} ×{}", status.label(), count));
                        ui.add_space(8.0);
                    }
                }
            });
            ui.separator();
            let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show_rows(ui, row_height, outcomes.len(), |ui, range| {
                    for outcome in &outcomes[range] {
                        ui.horizontal(|ui| {
                            ui.label(outcome.status.label());
                            ui.monospace(&outcome.fname);
                            ui.weak(format!("{}ms", outcome.elapsed_ms));
                            if !outcome.detail.is_empty() {
                                ui.weak(&outcome.detail);
                            }
                        });
                    }
                });
        });
    }

    fn stats_view(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("按 Mapper 的兼容性统计");
            ui.add_space(4.0);
            ui.weak("人工结果：✅正常 ⚠️小问题 ❌严重 💀无法运行；自动异常：黑屏/冻结/加载失败。");
            ui.add_space(6.0);
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("stats")
                        .striped(true)
                        .num_columns(10)
                        .show(ui, |ui| {
                            for title in [
                                "Mapper",
                                "名称",
                                "支持",
                                "ROM 数",
                                "已测",
                                "正常",
                                "小问题",
                                "严重",
                                "无法运行",
                                "自动异常",
                            ] {
                                ui.strong(title);
                            }
                            ui.end_row();
                            for row in &self.stats_rows {
                                if row.total == 0 {
                                    continue;
                                }
                                ui.label(
                                    row.mapper
                                        .map(|id| id.to_string())
                                        .unwrap_or_else(|| "NULL".into()),
                                );
                                ui.label(if row.name.is_empty() {
                                    if row.mapper.is_none() { "裸dump" } else { "?" }
                                } else {
                                    &row.name
                                });
                                ui.label(if row.supported { "✓" } else { "✗" });
                                ui.label(row.total.to_string());
                                ui.label(row.tested.to_string());
                                ui.label(row.ok.to_string());
                                ui.label(row.minor.to_string());
                                ui.label(row.major.to_string());
                                ui.label(row.broken.to_string());
                                ui.label(row.auto_bad.to_string());
                                ui.end_row();
                            }
                        });
                });
        });
    }

    fn settings_view(&mut self, ui: &mut Ui) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("设置");
            ui.add_space(6.0);
            egui::Grid::new("settings")
                .num_columns(3)
                .spacing([8.0, 8.0])
                .show(ui, |ui| {
                    path_row(ui, "数据库", &mut self.config.db_path, "nes-games.db");
                    path_row(ui, "ROM 包目录", &mut self.config.ever_dir, "ever");
                    path_row(ui, "结果库", &mut self.config.compat_db, "compat.db");
                    path_row(ui, "7-Zip", &mut self.config.seven_zip, "7zz");
                    path_row(ui, "FDS BIOS", &mut self.config.fds_bios, "disksys.rom");
                    path_row(ui, "界面字体", &mut self.config.font_path, "font.ttf");
                    ui.label("冒烟帧数");
                    ui.add(
                        egui::DragValue::new(&mut self.config.smoke_frames)
                            .range(30..=10000)
                            .speed(10),
                    );
                    ui.end_row();
                });
            ui.add_space(8.0);
            if !self.archive_error.is_empty() {
                ui.colored_label(Color32::RED, &self.archive_error);
            }
            if ui.button("保存并应用").clicked() {
                self.config.save();
                self.apply_settings();
            }
        });
    }

    fn apply_settings(&mut self) {
        match GameDb::open(&self.config.db_path) {
            Ok(db) => {
                self.db = Some(db);
                self.db_error.clear();
                self.mappers = self
                    .db
                    .as_ref()
                    .and_then(|db| db.mappers().ok())
                    .unwrap_or_default();
                self.types = self
                    .db
                    .as_ref()
                    .and_then(|db| db.types().ok())
                    .unwrap_or_default();
                self.refresh_games();
            }
            Err(error) => {
                self.db_error = format!("打开数据库失败: {error}");
            }
        }
        if let Some(seven_zip) = config::find_7zz(&self.config) {
            self.cache = Arc::new(ArchiveCache::new(seven_zip));
            self.archive_error.clear();
        } else {
            self.archive_error = "未找到 7-Zip".into();
        }
        install_cjk_font(&self.ctx, &self.config);
        self.stats_dirty = true;
        self.game_progress.clear();
        self.ui_message = "设置已保存".into();
    }

    // ---------- 播放视图 ----------

    fn play_view(&mut self, ui: &mut Ui) {
        // 底部：标记栏
        egui::Panel::bottom("mark-bar").show(ui, |ui| {
            ui.add_space(4.0);
            let session = self.play.clone();
            if let Some(session) = &session {
                ui.horizontal(|ui| {
                    ui.strong(&session.display);
                    ui.separator();
                    if ui.button(egui::RichText::new("✅ 正常").strong()).clicked() {
                        self.mark_current(CompatStatus::Ok);
                    }
                    if ui.button("⚠️ 小问题").clicked() {
                        self.mark_current(CompatStatus::Minor);
                    }
                    if ui.button("❌ 严重").clicked() {
                        self.mark_current(CompatStatus::Major);
                    }
                    if ui.button("💀 无法运行").clicked() {
                        self.mark_current(CompatStatus::Broken);
                    }
                    if ui.button("🎲 下一个未测试").clicked() {
                        self.next_untested();
                    }
                    if ui.button("⏹ 返回").clicked() {
                        self.exit_play();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("备注:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.note_buf)
                            .desired_width(ui.available_width() - 240.0)
                            .hint_text("可选：问题描述"),
                    );
                    ui.checkbox(&mut self.auto_advance, "标记后自动下一个");
                });
            }
            ui.add_space(2.0);
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let frame = self.player.snapshot_frame();
            if !self.player.is_running() {
                let message = self.player.message();
                ui.vertical_centered(|ui| {
                    ui.add_space(40.0);
                    ui.spinner();
                    ui.label(if message.is_empty() {
                        "加载中…".to_string()
                    } else {
                        message
                    });
                });
                return;
            }
            let avail = ui.available_size();
            let scale = (avail.x / nes_sim::FRAME_WIDTH as f32)
                .min(avail.y / nes_sim::FRAME_HEIGHT as f32)
                .max(0.5);
            let size = Vec2::new(
                nes_sim::FRAME_WIDTH as f32 * scale,
                nes_sim::FRAME_HEIGHT as f32 * scale,
            );
            let image = egui::Image::from_texture(&self.tex).fit_to_exact_size(size);
            let response = ui.centered_and_justified(|ui| ui.add(image)).response;

            // HUD（画布左上角）
            let hud = format!(
                "{}fps · 帧 {} · {} · {}{}",
                frame.fps,
                frame.frame_no,
                frame.tv_system,
                if frame.paused { "⏸ " } else { "" },
                if self.player.fast_forward() { "⏩" } else { "" },
            );
            let pos = response.rect.left_top() + Vec2::new(8.0, 6.0);
            ui.painter().text(
                pos,
                egui::Align2::LEFT_TOP,
                hud,
                egui::FontId::monospace(12.0),
                Color32::from_black_alpha(220),
            );

            // 控制条（画布下方）
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .button(if frame.paused { "▶ 继续" } else { "⏸ 暂停" })
                    .clicked()
                {
                    self.player.with_input(|input| input.toggle_pause = true);
                }
                if ui.button("🔄 复位").clicked() {
                    self.player.with_input(|input| input.reset = true);
                }
                if ui.button("📷 截图").clicked() {
                    self.save_screenshot();
                }
                if ui.button("F1 插退盘").clicked() {
                    self.player.with_input(|input| input.fds_toggle_insert = true);
                }
                if ui.button("F2 换盘面").clicked() {
                    self.player.with_input(|input| input.fds_next_side = true);
                }
                if let Some((current, total)) = frame.nsf_track {
                    if ui.button("◀ 曲目").clicked() {
                        self.player.with_input(|input| input.nsf_track_delta = -1);
                    }
                    ui.label(format!("{current}/{total}"));
                    if ui.button("曲目 ▶").clicked() {
                        self.player.with_input(|input| input.nsf_track_delta = 1);
                    }
                }
                ui.weak(
                    "键盘: 方向键 · Z=B · X=A · Tab=Select · Enter=Start · Space=快进 · P=暂停 · R=复位 · Esc=返回",
                );
            });
            ui.add_space(4.0);
            // 屏幕虚拟手柄（P1）
            ui.horizontal(|ui| {
                ui.label("P1:");
                let (a, b, sel, sta, up, down, left, right) = self.virtual_pad;
                self.virtual_pad = (
                    held_button(ui, "A", a),
                    held_button(ui, "B", b),
                    held_button(ui, "SEL", sel),
                    held_button(ui, "STA", sta),
                    held_button(ui, "▲", up),
                    held_button(ui, "▼", down),
                    held_button(ui, "◀", left),
                    held_button(ui, "▶", right),
                );
            });
        });
    }

    fn save_screenshot(&mut self) {
        let frame = self.player.snapshot_frame();
        let dir = config::project_root()
            .unwrap_or_default()
            .join("roms/compat-shots");
        let _ = std::fs::create_dir_all(&dir);
        let fname = self
            .play
            .as_ref()
            .map(|session| session.fname.clone())
            .unwrap_or_else(|| "frame".into());
        let stem: String = fname
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '.' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let path = dir.join(format!("{stem}_{}.ppm", frame.frame_no));
        match write_ppm(&path, frame.width, frame.height, &frame.rgba) {
            Ok(()) => self.ui_message = format!("截图已保存: {}", path.display()),
            Err(error) => self.ui_message = format!("截图失败: {error}"),
        }
    }
}

fn held_button(ui: &mut Ui, label: &str, _prev: bool) -> bool {
    let response = ui.add(egui::Button::new(label).small());
    response.is_pointer_button_down_on() || response.clicked()
}

fn pick_random(roms: &[RomRow]) -> Option<&RomRow> {
    let candidates: Vec<&RomRow> = roms
        .iter()
        .filter(|rom| rom.mapper.is_some() && !rom.md5.is_empty())
        .collect();
    if candidates.is_empty() {
        return Option::None;
    }
    let index = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as usize)
        .unwrap_or(0)
        % candidates.len();
    Some(candidates[index])
}

fn write_ppm(
    path: &std::path::Path,
    width: usize,
    height: usize,
    rgba: &[u8],
) -> std::io::Result<()> {
    let mut out = Vec::with_capacity(rgba.len() + 64);
    out.extend_from_slice(format!("P6\n{width} {height}\n255\n").as_bytes());
    for pixel in rgba.chunks_exact(4) {
        out.extend_from_slice(&pixel[..3]);
    }
    std::fs::write(path, out)
}

fn display_name(game: &GameListItem) -> String {
    let name = if game.name_cn.is_empty() {
        &game.name_en
    } else {
        &game.name_cn
    };
    if name.is_empty() {
        format!("#{}", game.id)
    } else {
        name.clone()
    }
}

fn mapper_desc(rom: &RomRow) -> String {
    match rom.mapper {
        Some(mapper) => {
            let name = rom
                .mapper_name
                .as_deref()
                .map(|n| format!(" {n}"))
                .unwrap_or_default();
            let flag = if rom.mapper_supported { "" } else { " ✗" };
            format!("{mapper}{name}{flag}")
        }
        Option::None if !rom.board.is_empty() => format!("UNIF {}", rom.board),
        Option::None => "裸dump".into(),
    }
}

fn human_size(size: i64) -> String {
    if size >= 1024 * 1024 {
        format!("{:.1}M", size as f64 / (1024.0 * 1024.0))
    } else if size >= 1024 {
        format!("{:.0}K", size as f64 / 1024.0)
    } else {
        format!("{size}B")
    }
}

fn path_row(ui: &mut Ui, label: &str, value: &mut PathBuf, name_filter: &str) {
    ui.label(label);
    let mut text = value.display().to_string();
    ui.add(egui::TextEdit::singleline(&mut text).desired_width(ui.available_width() - 80.0));
    if text != value.display().to_string() {
        *value = PathBuf::from(text);
    }
    if ui.small_button("…").clicked() {
        let dialog = rfd::FileDialog::new().set_file_name(name_filter);
        let picked = if label == "ROM 包目录" {
            dialog.pick_folder()
        } else {
            dialog.pick_file()
        };
        if let Some(path) = picked {
            *value = path;
        }
    }
    ui.end_row();
}

/// 安装 CJK 字体（egui 默认字体不含中文，缺失时界面中文显示为方块）。
/// CJK 字体设为 Proportional 的最高优先级：拉丁与中文用同一字体渲染，
/// 行高/基线一致，避免混排时英文下沉、中文上浮与按钮内边距不齐。
fn install_cjk_font(ctx: &egui::Context, config: &Config) {
    let candidates: Vec<PathBuf> = if !config.font_path.as_os_str().is_empty() {
        vec![config.font_path.clone()]
    } else {
        [
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/System/Library/Fonts/STHeiti Light.ttc",
            "C:/Windows/Fonts/msyh.ttc",
            "C:/Windows/Fonts/simhei.ttf",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        ]
        .iter()
        .map(PathBuf::from)
        .collect()
    };
    for candidate in candidates {
        let Ok(bytes) = std::fs::read(&candidate) else {
            continue;
        };
        // FontData::from_owned 不做格式校验，坏文件会在渲染期表现为无字形，
        // 因此按"设置后仍有 tofu"人工验证；这里不做进一步探测。
        ctx.add_font(egui::FontInsert::new(
            "cjk",
            egui::FontData::from_owned(bytes),
            vec![
                egui::InsertFontFamily {
                    family: egui::FontFamily::Proportional,
                    priority: egui::FontPriority::Highest,
                },
                egui::InsertFontFamily {
                    family: egui::FontFamily::Monospace,
                    // 等宽字体保持 ASCII 用内置 Hack（文件名/HUD 对齐），
                    // 仅作 CJK 回退
                    priority: egui::FontPriority::Lowest,
                },
            ],
        ));
        eprintln!("界面字体: {}", candidate.display());
        return;
    }
    eprintln!("未找到 CJK 系统字体，中文可能显示为方块（可在设置中指定字体文件）");
}
