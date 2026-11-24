//! `nes-games.db` 只读查询层：游戏检索、ROM/NSF 明细、mapper 字典。

use rusqlite::{Connection, OpenFlags};
use std::path::Path;

pub struct GameDb {
    conn: Connection,
}

/// 查询模型（部分字段供详情/后续视图使用）。
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct GameListItem {
    pub id: i64,
    pub name_cn: String,
    pub name_en: String,
    pub name_jp: String,
    pub company: String,
    pub release_year: i64,
    pub type1: String,
    pub romnum: i64,
    pub good: i64,
    pub playernum: i64,
}

/// 查询模型（部分字段供详情/后续视图使用）。
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct GameDetail {
    pub item: GameListItem,
    pub type2: String,
    pub release_date: String,
    pub info: String,
}

/// 查询模型（部分字段供详情/后续视图使用）。
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RomRow {
    pub id: i64,
    pub fname: String,
    pub md5: String,
    pub crc32: String,
    pub size: i64,
    pub mapper: Option<i64>,
    pub prg_kb: i64,
    pub chr_kb: i64,
    pub board: String,
    pub mapper_name: Option<String>,
    pub mapper_supported: bool,
    pub game_id: i64,
}

/// 查询模型（部分字段供详情/后续视图使用）。
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct NsfRow {
    pub id: i64,
    pub fname: String,
    pub md5: String,
    pub size: i64,
    pub title: String,
    pub artist: String,
    pub copyright: String,
    pub songs: i64,
    pub timing: String,
    pub chips: String,
    pub game_id: i64,
    pub game_name: String,
}

#[derive(Debug, Clone)]
pub struct MapperInfo {
    pub id: i64,
    pub name: String,
    pub supported: bool,
    pub rom_count: i64,
}

#[derive(Debug, Clone, Default)]
pub struct GameFilter {
    pub text: String,
    pub mapper: Option<i64>,
    pub type1: Option<String>,
}

impl GameDb {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        Ok(Self { conn })
    }

    /// mapper 字典 + 各 mapper 的 ROM 数量。
    pub fn mappers(&self) -> rusqlite::Result<Vec<MapperInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT m.id, COALESCE(m.name,''), COALESCE(m.supported,0),
                    (SELECT COUNT(*) FROM rom r WHERE r.mapper = m.id)
             FROM mapper_name m ORDER BY m.id",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(MapperInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                supported: row.get::<_, i64>(2)? != 0,
                rom_count: row.get(3)?,
            })
        })?;
        rows.collect()
    }

    pub fn types(&self) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT type1 FROM game WHERE type1 IS NOT NULL ORDER BY type1")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect()
    }

    fn game_columns(row: &rusqlite::Row<'_>) -> rusqlite::Result<GameListItem> {
        Ok(GameListItem {
            id: row.get(0)?,
            name_cn: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
            name_en: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            name_jp: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            company: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            release_year: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
            type1: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
            romnum: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
            good: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
            playernum: row.get::<_, Option<i64>>(9)?.unwrap_or(0),
        })
    }

    /// 游戏检索：文本（中/英/日文名）+ mapper / 类型 筛选，按 ROM 数降序。
    pub fn search_games(&self, filter: &GameFilter) -> rusqlite::Result<Vec<GameListItem>> {
        let mut sql = String::from(
            "SELECT g.id, g.name_cn2, g.name_en2, g.name_jp, g.company, g.release0,
                    g.type1, g.romnum, g.good, g.playernum
             FROM game g WHERE 1=1",
        );
        let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        let text = filter.text.trim();
        if !text.is_empty() {
            sql.push_str(" AND (g.name_cn2 LIKE ? ESCAPE '\\' OR g.name_cn LIKE ? ESCAPE '\\' OR g.name_en2 LIKE ? ESCAPE '\\' OR g.name_en LIKE ? ESCAPE '\\' OR g.name_jp LIKE ? ESCAPE '\\')");
            let pattern = format!("%{}%", escape_like(text));
            for _ in 0..5 {
                params.push(Box::new(pattern.clone()));
            }
        }
        if let Some(mapper) = filter.mapper {
            sql.push_str(
                " AND EXISTS (SELECT 1 FROM rom r WHERE r.game_id = g.id AND r.mapper = ?)",
            );
            params.push(Box::new(mapper));
        }
        if let Some(type1) = &filter.type1 {
            sql.push_str(" AND g.type1 = ?");
            params.push(Box::new(type1.clone()));
        }
        sql.push_str(" ORDER BY g.romnum DESC, g.id LIMIT 20000");
        let mut stmt = self.conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), Self::game_columns)?;
        rows.collect()
    }

    pub fn game_detail(&self, id: i64) -> rusqlite::Result<Option<GameDetail>> {
        let found = self
            .conn
            .query_row(
                "SELECT g.id, g.name_cn2, g.name_en2, g.name_jp, g.company, g.release0,
                        g.type1, g.romnum, g.good, g.playernum, g.type2, g.release1, g.info
                 FROM game g WHERE g.id = ?1",
                [id],
                |row| {
                    let item = Self::game_columns(row)?;
                    Ok(GameDetail {
                        item,
                        type2: row.get::<_, Option<String>>(10)?.unwrap_or_default(),
                        release_date: row.get::<_, Option<String>>(11)?.unwrap_or_default(),
                        info: row.get::<_, Option<String>>(12)?.unwrap_or_default(),
                    })
                },
            )
            .map(Some);
        match found {
            Ok(detail) => Ok(detail),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// 任取一个有 NSF 记录的 game_id（测试用：NSF 解压验证）。
    #[cfg(test)]
    pub fn first_nsf_game_id(&self) -> rusqlite::Result<Option<i64>> {
        let found = self.conn.query_row(
            "SELECT game_id FROM nsf WHERE game_id IS NOT NULL LIMIT 1",
            [],
            |row| row.get::<_, i64>(0),
        );
        match found {
            Ok(id) => Ok(Some(id)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn roms_for_game(&self, game_id: i64) -> rusqlite::Result<Vec<RomRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.fname, COALESCE(r.md5,''), COALESCE(r.crc32,''), COALESCE(r.size,0),
                    r.mapper, COALESCE(r.prg_kb,0), COALESCE(r.chr_kb,0), COALESCE(r.board,''),
                    m.name, COALESCE(m.supported,0), r.game_id
             FROM rom r LEFT JOIN mapper_name m ON m.id = r.mapper
             WHERE r.game_id = ?1 ORDER BY r.fname",
        )?;
        let rows = stmt.query_map([game_id], Self::rom_columns)?;
        rows.collect()
    }

    fn rom_columns(row: &rusqlite::Row<'_>) -> rusqlite::Result<RomRow> {
        Ok(RomRow {
            id: row.get(0)?,
            fname: row.get(1)?,
            md5: row.get(2)?,
            crc32: row.get(3)?,
            size: row.get(4)?,
            mapper: row.get(5)?,
            prg_kb: row.get(6)?,
            chr_kb: row.get(7)?,
            board: row.get(8)?,
            mapper_name: row.get(9)?,
            mapper_supported: row.get::<_, Option<i64>>(10)?.is_some_and(|v| v != 0),
            game_id: row.get(11)?,
        })
    }

    pub fn nsfs_for_game(&self, game_id: i64) -> rusqlite::Result<Vec<NsfRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.fname, COALESCE(n.md5,''), COALESCE(n.size,0),
                    COALESCE(n.title,''), COALESCE(n.artist,''), COALESCE(n.copyright,''),
                    COALESCE(n.songs,0), COALESCE(n.timing,''), COALESCE(n.chips,''), n.game_id,
                    COALESCE(g.name_cn2,'') || ' ' || COALESCE(g.name_en2,'')
             FROM nsf n LEFT JOIN game g ON g.id = n.game_id
             WHERE n.game_id = ?1 ORDER BY n.fname LIMIT 500",
        )?;
        let rows = stmt.query_map([game_id], Self::nsf_columns)?;
        rows.collect()
    }

    fn nsf_columns(row: &rusqlite::Row<'_>) -> rusqlite::Result<NsfRow> {
        Ok(NsfRow {
            id: row.get(0)?,
            fname: row.get(1)?,
            md5: row.get(2)?,
            size: row.get(3)?,
            title: row.get(4)?,
            artist: row.get(5)?,
            copyright: row.get(6)?,
            songs: row.get(7)?,
            timing: row.get(8)?,
            chips: row.get(9)?,
            game_id: row.get::<_, Option<i64>>(10)?.unwrap_or(0),
            game_name: row.get(11)?,
        })
    }

    /// NSF 检索（曲名/艺术家/游戏名/文件名），带数量上限。
    pub fn search_nsfs(&self, text: &str, limit: i64) -> rusqlite::Result<Vec<NsfRow>> {
        let text = text.trim();
        let (sql, param): (String, String) = if text.is_empty() {
            (
                "SELECT n.id, n.fname, COALESCE(n.md5,''), COALESCE(n.size,0),
                        COALESCE(n.title,''), COALESCE(n.artist,''), COALESCE(n.copyright,''),
                        COALESCE(n.songs,0), COALESCE(n.timing,''), COALESCE(n.chips,''), n.game_id,
                        COALESCE(g.name_cn2,'') || ' ' || COALESCE(g.name_en2,'')
                 FROM nsf n LEFT JOIN game g ON g.id = n.game_id
                 ORDER BY n.id LIMIT ?2"
                    .into(),
                String::new(),
            )
        } else {
            (
                "SELECT n.id, n.fname, COALESCE(n.md5,''), COALESCE(n.size,0),
                        COALESCE(n.title,''), COALESCE(n.artist,''), COALESCE(n.copyright,''),
                        COALESCE(n.songs,0), COALESCE(n.timing,''), COALESCE(n.chips,''), n.game_id,
                        COALESCE(g.name_cn2,'') || ' ' || COALESCE(g.name_en2,'')
                 FROM nsf n LEFT JOIN game g ON g.id = n.game_id
                 WHERE n.title LIKE ?1 ESCAPE '\\' OR n.artist LIKE ?1 ESCAPE '\\'
                    OR n.fname LIKE ?1 ESCAPE '\\'
                    OR g.name_cn2 LIKE ?1 ESCAPE '\\' OR g.name_en2 LIKE ?1 ESCAPE '\\'
                 ORDER BY n.id LIMIT ?2"
                    .into(),
                format!("%{}%", escape_like(text)),
            )
        };
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params![param, limit], Self::nsf_columns)?;
        rows.collect()
    }

    /// 全库 ROM 索引（game_id, mapper, md5），供内存联查测试进度。
    /// 139k 行一次载入约 50ms。
    pub fn rom_index(&self) -> rusqlite::Result<Vec<(i64, Option<i64>, String)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT game_id, mapper, COALESCE(md5,'') FROM rom")?;
        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                row.get(1)?,
                row.get(2)?,
            ))
        })?;
        rows.collect()
    }

    /// 随机抽一条未测试 ROM（在候选池里跳过已测 key；`tested` 为结果库 key 集合）。
    pub fn random_untested_rom(
        &self,
        tested: &std::collections::HashSet<String>,
        mapper: Option<i64>,
    ) -> rusqlite::Result<Option<(RomRow, String)>> {
        let mut sql = String::from(
            "SELECT r.id, r.fname, COALESCE(r.md5,''), COALESCE(r.crc32,''), COALESCE(r.size,0),
                    r.mapper, COALESCE(r.prg_kb,0), COALESCE(r.chr_kb,0), COALESCE(r.board,''),
                    m.name, COALESCE(m.supported,0), r.game_id,
                    COALESCE(g.name_cn2,'') || ' ' || COALESCE(g.name_en2,'')
             FROM rom r
             LEFT JOIN mapper_name m ON m.id = r.mapper
             LEFT JOIN game g ON g.id = r.game_id
             WHERE r.mapper IS NOT NULL",
        );
        if mapper.is_some() {
            sql.push_str(" AND r.mapper = ?");
        }
        sql.push_str(" ORDER BY RANDOM() LIMIT 64");
        let mut stmt = self.conn.prepare(&sql)?;
        let to_sql: Vec<&dyn rusqlite::types::ToSql> = match mapper.as_ref() {
            Some(value) => vec![value],
            Option::None => vec![],
        };
        let rows = stmt.query_map(to_sql.as_slice(), |row| {
            Ok((Self::rom_columns(row)?, row.get::<_, String>(12)?))
        })?;
        for row in rows {
            let (rom, game_name) = row?;
            if !rom.md5.is_empty() && !tested.contains(&rom.md5) {
                return Ok(Some((rom, game_name)));
            }
        }
        Ok(None)
    }

    /// mapper 范围内的 ROM 候选（冒烟测试用）：可选未测过滤在内存做。
    pub fn roms_by_mapper(
        &self,
        mapper: i64,
        limit: i64,
    ) -> rusqlite::Result<Vec<(RomRow, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT r.id, r.fname, COALESCE(r.md5,''), COALESCE(r.crc32,''), COALESCE(r.size,0),
                    r.mapper, COALESCE(r.prg_kb,0), COALESCE(r.chr_kb,0), COALESCE(r.board,''),
                    m.name, COALESCE(m.supported,0), r.game_id,
                    COALESCE(g.name_cn2,'') || ' ' || COALESCE(g.name_en2,'')
             FROM rom r
             LEFT JOIN mapper_name m ON m.id = r.mapper
             LEFT JOIN game g ON g.id = r.game_id
             WHERE r.mapper = ?1
             ORDER BY RANDOM()
             LIMIT ?2",
        )?;
        let rows = stmt.query_map([mapper, limit], |row| {
            Ok((Self::rom_columns(row)?, row.get::<_, String>(12)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

/// LIKE 转义：用户输入中的 % _ 按字面匹配。
fn escape_like(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_db() -> Option<GameDb> {
        let path = Path::new("roms/nes-games.db");
        path.is_file()
            .then(|| GameDb::open(path).expect("open nes-games.db"))
    }

    #[test]
    fn escape_like_escapes_wildcards() {
        assert_eq!(escape_like("a%b_c"), "a\\%b\\_c");
    }

    #[test]
    fn search_and_detail_roundtrip() {
        let Some(db) = open_db() else {
            eprintln!("skip: roms/nes-games.db 不可用");
            return;
        };
        let games = db
            .search_games(&GameFilter {
                text: "Contra".into(),
                ..Default::default()
            })
            .expect("search");
        assert!(!games.is_empty(), "应能搜到 Contra");
        let detail = db.game_detail(games[0].id).expect("detail");
        assert!(detail.is_some());
        let roms = db.roms_for_game(games[0].id).expect("roms");
        assert!(!roms.is_empty(), "Contra 应有 ROM 记录");
        assert!(roms.iter().all(|rom| rom.game_id == games[0].id));
    }

    #[test]
    fn mapper_stats_and_rom_index() {
        let Some(db) = open_db() else {
            eprintln!("skip: roms/nes-games.db 不可用");
            return;
        };
        let mappers = db.mappers().expect("mappers");
        assert!(!mappers.is_empty());
        let index = db.rom_index().expect("rom index");
        assert!(index.len() > 100_000);
    }
}
