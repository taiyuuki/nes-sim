//! `roms/游戏rom/ever/<game_id>.7z` 按需访问：
//! 只用 `7zz l` / `7zz e -so` 流式读取单个文件，从不解压整个包到磁盘。

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEntry {
    /// 包内相对路径（含 `#音乐/` 等前缀）
    pub path: String,
    pub size: u64,
}

/// 每个游戏的包内条目列表缓存。
#[derive(Default)]
pub struct ArchiveCache {
    seven_zip: PathBuf,
    entries: Mutex<HashMap<i64, Vec<ArchiveEntry>>>,
}

impl ArchiveCache {
    pub fn new(seven_zip: PathBuf) -> Self {
        Self {
            seven_zip,
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn archive_path(ever_dir: &Path, game_id: i64) -> PathBuf {
        ever_dir.join(format!("{game_id}.7z"))
    }

    /// 列出包内文件（目录已剔除），结果缓存。
    pub fn list(&self, ever_dir: &Path, game_id: i64) -> Result<Vec<ArchiveEntry>, String> {
        if let Some(cached) = self.entries.lock().unwrap().get(&game_id) {
            return Ok(cached.clone());
        }
        let archive = Self::archive_path(ever_dir, game_id);
        if !archive.is_file() {
            return Err(format!("ROM 包不存在: {}", archive.display()));
        }
        let output = Command::new(&self.seven_zip)
            .arg("l")
            .arg("-slt")
            .arg("-y")
            .arg("-ba")
            .arg(&archive)
            .output()
            .map_err(|error| format!("启动 7z 失败: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "7z 列表失败 ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let text = String::from_utf8_lossy(&output.stdout);
        let entries = parse_slt_listing(&text);
        self.entries
            .lock()
            .unwrap()
            .insert(game_id, entries.clone());
        Ok(entries)
    }

    /// 流式解压包内单个文件到内存。
    /// 注意 7z 把名称当通配模式，含 `*`/`?` 的文件名可能匹配失败（GoodNES 名极少含）。
    pub fn extract(&self, ever_dir: &Path, game_id: i64, inner: &str) -> Result<Vec<u8>, String> {
        let archive = Self::archive_path(ever_dir, game_id);
        if !archive.is_file() {
            return Err(format!("ROM 包不存在: {}", archive.display()));
        }
        let mut child = Command::new(&self.seven_zip)
            .arg("e")
            .arg("-so")
            .arg("-y")
            .arg("-bso0")
            .arg(&archive)
            .arg(inner)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("启动 7z 失败: {error}"))?;
        let mut bytes = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            stdout
                .take(1024 * 1024 * 64)
                .read_to_end(&mut bytes)
                .map_err(|error| format!("读取解压输出失败: {error}"))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| format!("等待 7z 失败: {error}"))?;
        if !output.status.success() || bytes.is_empty() {
            return Err(format!(
                "解压 {inner} 失败 ({}): {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(bytes)
    }

    /// 按数据库文件名解压：列出包内条目 → 归一化匹配 → 流式解压。
    /// NSF 优先匹配 `#音乐/` 目录。
    pub fn extract_by_name(
        &self,
        ever_dir: &Path,
        game_id: i64,
        db_fname: &str,
        nsf: bool,
    ) -> Result<Vec<u8>, String> {
        let entries = self.list(ever_dir, game_id)?;
        let entry =
            match_entry(&entries, db_fname, nsf).ok_or_else(|| format!("包内未找到 {db_fname}"))?;
        self.extract(ever_dir, game_id, &entry.path)
    }
}

/// 解析 `7zz l -slt -ba` 的技术列表：以空行分隔的 `键 = 值` 块。
fn parse_slt_listing(text: &str) -> Vec<ArchiveEntry> {
    let mut entries = Vec::new();
    let mut path: Option<String> = Option::None;
    let mut size: u64 = 0;
    let mut is_dir = false;
    let mut flush = |path: &mut Option<String>, size: &mut u64, is_dir: &mut bool| {
        if let Some(path) = path.take()
            && !path.is_empty()
            && !*is_dir
        {
            entries.push(ArchiveEntry { path, size: *size });
        }
        *size = 0;
        *is_dir = false;
    };
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            flush(&mut path, &mut size, &mut is_dir);
            continue;
        }
        if let Some((key, value)) = line.split_once(" = ") {
            let value = value.trim();
            match key.trim() {
                "Path" => {
                    flush(&mut path, &mut size, &mut is_dir);
                    path = Some(value.to_string());
                }
                "Size" => {
                    size = value.parse().unwrap_or(0);
                }
                "Folder" => {
                    is_dir = value.eq_ignore_ascii_case("+");
                }
                "Attributes" => {
                    // 7z 目录条目属性含 D
                    is_dir |= value.contains('D');
                }
                _ => {}
            }
        }
    }
    flush(&mut path, &mut size, &mut is_dir);
    entries
}

/// 文件名归一化：小写 + 去目录前缀，用于数据库 fname 与包内条目互相匹配。
pub fn normalize_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    base.trim().to_lowercase()
}

/// 在包内条目中查找数据库文件名对应的条目。
/// NSF 优先匹配 `#音乐/` 前缀（包内音乐目录），否则按归一化文件名比对。
pub fn match_entry<'a>(
    entries: &'a [ArchiveEntry],
    db_fname: &str,
    prefer_music_dir: bool,
) -> Option<&'a ArchiveEntry> {
    let target = normalize_name(db_fname);
    let mut by_name: Vec<&ArchiveEntry> = entries
        .iter()
        .filter(|entry| normalize_name(&entry.path) == target)
        .collect();
    if by_name.len() > 1 && prefer_music_dir {
        by_name.sort_by_key(|entry| !entry.path.contains('#'));
    }
    if by_name.is_empty() {
        // 兜底：包内条目名包含数据库名（或反之），处理前缀差异
        by_name = entries
            .iter()
            .filter(|entry| {
                let name = normalize_name(&entry.path);
                name.starts_with(&target) || target.starts_with(&name)
            })
            .collect();
    }
    by_name.first().copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ever_dir() -> Option<PathBuf> {
        let dir = PathBuf::from("roms/游戏rom/ever");
        dir.is_dir().then_some(dir)
    }

    fn seven_zip() -> Option<PathBuf> {
        crate::config::find_7zz(&crate::config::Config::default())
    }

    #[test]
    fn parse_slt_listing_parses_blocks() {
        let text = "\n\nPath = Game (J) [!].nes\nSize = 65552\nFolder = -\nAttributes = A\n\nPath = #音乐\nSize = 0\nFolder = +\nAttributes = D_\n\nPath = #音乐/BGM.nsf\nSize = 20480\nFolder = -\n\n";
        let entries = parse_slt_listing(text);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].path, "Game (J) [!].nes");
        assert_eq!(entries[0].size, 65552);
        assert_eq!(entries[1].path, "#音乐/BGM.nsf");
    }

    #[test]
    fn match_entry_prefers_music_dir_for_nsf() {
        let entries = [
            ArchiveEntry {
                path: "bgm.nsf".into(),
                size: 1,
            },
            ArchiveEntry {
                path: "#音乐/bgm.nsf".into(),
                size: 2,
            },
        ];
        let matched = match_entry(&entries, "BGM.NSF", true).unwrap();
        assert_eq!(matched.path, "#音乐/bgm.nsf");
        let matched = match_entry(&entries, "BGM.NSF", false).unwrap();
        assert_eq!(matched.path, "bgm.nsf");
    }

    #[test]
    fn match_entry_normalizes_case_and_dirs() {
        let entries = [ArchiveEntry {
            path: "ROM/Super Mario Bros. (W) [!].nes".into(),
            size: 10,
        }];
        let matched = match_entry(&entries, "super mario bros. (w) [!].nes", false).unwrap();
        assert_eq!(matched.size, 10);
    }

    /// 真实包往返：列出 → 匹配 → 解压首字节 "NES\x1A"。缺包环境自动跳过。
    #[test]
    fn real_archive_roundtrip() {
        let (Some(dir), Some(seven_zip)) = (ever_dir(), seven_zip()) else {
            eprintln!("skip: roms/游戏rom/ever 或 7z 不可用");
            return;
        };
        let cache = ArchiveCache::new(seven_zip);
        // 任取一个存在 rom 记录的游戏包
        let game_id: i64 = 532;
        let entries = cache.list(&dir, game_id).expect("list archive");
        assert!(!entries.is_empty(), "包内应有文件");
        let nes = entries
            .iter()
            .find(|entry| entry.path.to_lowercase().ends_with(".nes"))
            .expect("包内应有 .nes");
        let bytes = cache.extract(&dir, game_id, &nes.path).expect("extract");
        assert_eq!(&bytes[..4], b"NES\x1a", "iNES 魔数");
    }

    /// 真实包 NSF 解压：按 nsf.fname 匹配（优先 `#音乐/`），首字节 "NESM\x1A"。
    #[test]
    fn real_archive_nsf_extraction() {
        let (Some(dir), Some(seven_zip)) = (ever_dir(), seven_zip()) else {
            eprintln!("skip: roms/游戏rom/ever 或 7z 不可用");
            return;
        };
        let Ok(db) = crate::db::GameDb::open(std::path::Path::new("roms/nes-games.db")) else {
            eprintln!("skip: roms/nes-games.db 不可用");
            return;
        };
        // 找一个既有 NSF 记录、包也存在的游戏
        let game_id: Option<i64> = db
            .first_nsf_game_id()
            .ok()
            .flatten()
            .filter(|id| dir.join(format!("{id}.7z")).is_file());
        let Some(game_id) = game_id else {
            eprintln!("skip: 找不到含 NSF 的本地游戏包");
            return;
        };
        let cache = ArchiveCache::new(seven_zip);
        let nsfs = db.nsfs_for_game(game_id).expect("nsf rows");
        assert!(!nsfs.is_empty());
        let nsf = &nsfs[0];
        let bytes = cache
            .extract_by_name(&dir, game_id, &nsf.fname, true)
            .expect("extract nsf");
        assert_eq!(&bytes[..5], b"NESM\x1a", "NSF 魔数: {}", nsf.fname);
    }
}
