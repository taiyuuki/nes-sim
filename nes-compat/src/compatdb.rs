//! 兼容性结果库 `roms/compat.db`：与 nes-games.db（只读构建产物）分离，
//! 按 ROM/NSF 整文件 MD5 关联测试记录。

use rusqlite::{Connection, params};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// 测试结果状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompatStatus {
    /// 正常（人工确认）
    Ok,
    /// 有小问题但可玩（人工）
    Minor,
    /// 严重问题（人工）
    Major,
    /// 无法运行（人工）
    Broken,
    /// 加载失败（自动：解压后核心拒绝加载）
    LoadError,
    /// 跳过（自动：解压失败 / FDS 无 BIOS 等）
    Skip,
    /// 疑似黑屏（自动：末帧几乎无色彩变化）
    Blank,
    /// 疑似画面冻结（自动：帧推进但画面不变）
    Frozen,
    /// 冒烟通过（自动：能跑且画面有变化，供人工复核）
    SmokeOk,
}

impl CompatStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Minor => "minor",
            Self::Major => "major",
            Self::Broken => "broken",
            Self::LoadError => "load_error",
            Self::Skip => "skip",
            Self::Blank => "blank",
            Self::Frozen => "frozen",
            Self::SmokeOk => "smoke_ok",
        }
    }

    /// 结果来源是人工标记（优先于自动记录）。
    pub fn is_manual(self) -> bool {
        matches!(self, Self::Ok | Self::Minor | Self::Major | Self::Broken)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ok => "✅ 正常",
            Self::Minor => "⚠️ 小问题",
            Self::Major => "❌ 严重",
            Self::Broken => "💀 无法运行",
            Self::LoadError => "⛔ 加载失败",
            Self::Skip => "⏭️ 跳过",
            Self::Blank => "📷 疑似黑屏",
            Self::Frozen => "🧊 疑似冻结",
            Self::SmokeOk => "🟡 冒烟通过",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "ok" => Self::Ok,
            "minor" => Self::Minor,
            "major" => Self::Major,
            "broken" => Self::Broken,
            "load_error" => Self::LoadError,
            "skip" => Self::Skip,
            "blank" => Self::Blank,
            "frozen" => Self::Frozen,
            "smoke_ok" => Self::SmokeOk,
            _ => return None,
        })
    }
}

/// 条目种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Rom,
    Nsf,
}

impl ItemKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rom => "rom",
            Self::Nsf => "nsf",
        }
    }
}

/// 查询结果模型（部分字段供统计/导出等后续使用）。
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ResultRow {
    pub key: String,
    pub kind: ItemKind,
    pub fname: String,
    pub game_id: i64,
    pub status: CompatStatus,
    pub note: String,
    pub manual: bool,
    pub updated_at: String,
}

/// 打开（必要时创建）结果库，启用 WAL。
pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS result (
            key        TEXT PRIMARY KEY,
            kind       TEXT NOT NULL,
            fname      TEXT NOT NULL DEFAULT '',
            game_id    INTEGER,
            status     TEXT NOT NULL,
            note       TEXT NOT NULL DEFAULT '',
            source     TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_result_game ON result(game_id);
        CREATE INDEX IF NOT EXISTS idx_result_status ON result(status);
        CREATE TABLE IF NOT EXISTS smoke_log (
            key              TEXT PRIMARY KEY,
            frames           INTEGER,
            elapsed_ms       INTEGER,
            distinct_colors  INTEGER,
            audio_energy     REAL,
            frozen           INTEGER,
            detail           TEXT,
            updated_at       TEXT NOT NULL
        );",
    )?;
    Ok(conn)
}

fn now_iso() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // 简单 UTC 时间戳（本地时区换算交给查看者）
    format!("+{secs}")
}

/// 写入结果；人工结果不会被自动（smoke）结果覆盖。
pub fn set_result(
    conn: &Connection,
    key: &str,
    kind: ItemKind,
    fname: &str,
    game_id: i64,
    status: CompatStatus,
    note: &str,
) -> rusqlite::Result<()> {
    let manual = status.is_manual();
    let existing_manual: Option<String> = conn
        .query_row(
            "SELECT source FROM result WHERE key = ?1",
            params![key],
            |row| row.get::<_, String>(0),
        )
        .map(Some)
        .or_else(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(other),
        })?;
    if existing_manual.as_deref() == Some("manual") && !manual {
        return Ok(());
    }
    conn.execute(
        "INSERT INTO result (key, kind, fname, game_id, status, note, source, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(key) DO UPDATE SET
            kind = excluded.kind,
            fname = excluded.fname,
            game_id = excluded.game_id,
            status = excluded.status,
            note = excluded.note,
            source = excluded.source,
            updated_at = excluded.updated_at",
        params![
            key,
            kind.as_str(),
            fname,
            game_id,
            status.as_str(),
            note,
            if manual { "manual" } else { "smoke" },
            now_iso()
        ],
    )?;
    Ok(())
}

/// 载入全部结果（key -> 行），供列表/统计在内存中联查。
pub fn load_all(
    conn: &Connection,
) -> rusqlite::Result<std::collections::HashMap<String, ResultRow>> {
    let mut stmt = conn.prepare(
        "SELECT key, kind, fname, game_id, status, note, source, updated_at FROM result",
    )?;
    let rows = stmt.query_map([], |row| {
        let status: String = row.get(4)?;
        let source: String = row.get(6)?;
        let key: String = row.get(0)?;
        Ok((
            key.clone(),
            ResultRow {
                key,
                kind: if row.get::<_, String>(1)?.as_str() == "nsf" {
                    ItemKind::Nsf
                } else {
                    ItemKind::Rom
                },
                fname: row.get(2)?,
                game_id: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
                status: CompatStatus::from_str(&status).unwrap_or(CompatStatus::Skip),
                note: row.get(5)?,
                manual: source == "manual",
                updated_at: row.get(7)?,
            },
        ))
    })?;
    let mut map = std::collections::HashMap::new();
    for row in rows {
        let (key, value) = row?;
        map.insert(key, value);
    }
    Ok(map)
}

/// 写入冒烟测试原始指标。
#[allow(clippy::too_many_arguments)]
pub fn log_smoke(
    conn: &Connection,
    key: &str,
    frames: u32,
    elapsed_ms: u64,
    distinct_colors: usize,
    audio_energy: f64,
    frozen: bool,
    detail: &str,
) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO smoke_log
            (key, frames, elapsed_ms, distinct_colors, audio_energy, frozen, detail, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(key) DO UPDATE SET
            frames = excluded.frames,
            elapsed_ms = excluded.elapsed_ms,
            distinct_colors = excluded.distinct_colors,
            audio_energy = excluded.audio_energy,
            frozen = excluded.frozen,
            detail = excluded.detail,
            updated_at = excluded.updated_at",
        params![
            key,
            frames,
            elapsed_ms as i64,
            distinct_colors as i64,
            audio_energy,
            frozen,
            detail,
            now_iso()
        ],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_db(name: &str) -> Connection {
        let path =
            std::env::temp_dir().join(format!("nes-compat-test-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        open(&path).expect("open temp db")
    }

    #[test]
    fn manual_result_not_overwritten_by_smoke() {
        let conn = temp_db("manual");
        set_result(
            &conn,
            "k1",
            ItemKind::Rom,
            "a.nes",
            7,
            CompatStatus::Ok,
            "人工",
        )
        .unwrap();
        set_result(
            &conn,
            "k1",
            ItemKind::Rom,
            "a.nes",
            7,
            CompatStatus::Blank,
            "自动",
        )
        .unwrap();
        let map = load_all(&conn).unwrap();
        let row = &map["k1"];
        assert_eq!(row.status, CompatStatus::Ok);
        assert!(row.manual);

        // 无人工记录时自动结果正常写入，且可被后续人工覆盖
        set_result(
            &conn,
            "k2",
            ItemKind::Rom,
            "b.nes",
            8,
            CompatStatus::LoadError,
            "mapper 999",
        )
        .unwrap();
        set_result(
            &conn,
            "k2",
            ItemKind::Rom,
            "b.nes",
            8,
            CompatStatus::Broken,
            "人工复核",
        )
        .unwrap();
        let map = load_all(&conn).unwrap();
        assert_eq!(map["k2"].status, CompatStatus::Broken);
        assert!(map["k2"].manual);
    }

    #[test]
    fn load_all_returns_map() {
        let conn = temp_db("load_all");
        set_result(&conn, "ka", ItemKind::Rom, "a.nes", 1, CompatStatus::Ok, "").unwrap();
        set_result(
            &conn,
            "kb",
            ItemKind::Nsf,
            "b.nsf",
            2,
            CompatStatus::SmokeOk,
            "",
        )
        .unwrap();
        let map = load_all(&conn).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map["ka"].status, CompatStatus::Ok);
        assert_eq!(map["kb"].kind, ItemKind::Nsf);
    }
}
