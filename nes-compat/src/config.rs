//! 客户端配置：持久化到 `roms/compat.toml`（roms/ 已被 gitignore）。

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// nes-games.db 路径（相对路径按工作目录解析）
    pub db_path: PathBuf,
    /// 游戏rom/ever 目录（每游戏一个 <id>.7z）
    pub ever_dir: PathBuf,
    /// 兼容性结果库路径
    pub compat_db: PathBuf,
    /// 7-Zip 可执行文件；留空则自动探测（7zz / 7z）
    pub seven_zip: PathBuf,
    /// FDS BIOS (disksys.rom) 路径；留空则 FDS 无法运行
    pub fds_bios: PathBuf,
    /// 自定义 CJK 字体路径；留空则扫描系统字体
    pub font_path: PathBuf,
    /// 冒烟测试默认帧数
    pub smoke_frames: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("roms/nes-games.db"),
            ever_dir: PathBuf::from("roms/游戏rom/ever"),
            compat_db: PathBuf::from("roms/compat.db"),
            seven_zip: PathBuf::new(),
            fds_bios: PathBuf::new(),
            font_path: PathBuf::new(),
            smoke_frames: 300,
        }
    }
}

impl Config {
    /// 配置文件位置：项目根下 `roms/compat.toml`；找不到根时退回工作目录。
    pub fn path() -> PathBuf {
        let local = PathBuf::from("roms/compat.toml");
        match project_root() {
            Some(root) => root.join(local),
            Option::None => local,
        }
    }

    pub fn load() -> Self {
        let path = Self::path();
        let mut config = match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str(&text) {
                Ok(config) => config,
                Err(error) => {
                    eprintln!("配置解析失败，使用默认值: {error} ({})", path.display());
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        };
        config.anchor_relative_paths();
        config
    }

    /// 相对路径锚定到项目根：从其他目录启动（如双击二进制）时
    /// 仍能找到 roms/ 数据。绝对路径不受影响。
    fn anchor_relative_paths(&mut self) {
        let Some(root) = project_root() else {
            return;
        };
        for field in [&mut self.db_path, &mut self.ever_dir, &mut self.compat_db] {
            if field.is_relative() {
                *field = root.join(&*field);
            }
        }
    }

    pub fn save(&self) {
        let path = Self::path();
        match toml::to_string_pretty(self) {
            Ok(text) => {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if let Err(error) = std::fs::write(&path, text) {
                    eprintln!("配置保存失败: {error} ({})", path.display());
                }
            }
            Err(error) => eprintln!("配置序列化失败: {error}"),
        }
    }

    /// 解析可能存在的 FDS BIOS；未配置或读取失败返回 None。
    pub fn fds_bios_bytes(&self) -> Option<Vec<u8>> {
        if self.fds_bios.as_os_str().is_empty() {
            return None;
        }
        std::fs::read(&self.fds_bios).ok()
    }
}

/// 项目根目录发现：CWD 及其祖先中存在 `roms/nes-games.db` 文件者；
/// 否则回退到编译期工作区根（本工具为本地 publish=false 项目）。
/// 判定用数据库文件而非 `roms/` 目录，避免被历史误运行产生的空目录劫持。
pub fn project_root() -> Option<PathBuf> {
    if let Ok(cwd) = std::env::current_dir() {
        for dir in cwd.ancestors() {
            if dir.join("roms").join("nes-games.db").is_file() {
                return Some(dir.to_path_buf());
            }
        }
    }
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()?
        .to_path_buf();
    root.join("roms")
        .join("nes-games.db")
        .is_file()
        .then_some(root)
}

/// 探测 7-Zip 可执行文件：配置优先，其次 PATH 与常见安装位置。
pub fn find_7zz(config: &Config) -> Option<PathBuf> {
    if !config.seven_zip.as_os_str().is_empty() {
        let path = &config.seven_zip;
        return path.is_file().then(|| path.clone());
    }
    for name in ["7zz", "7z"] {
        if let Some(found) = find_in_path(name) {
            return Some(found);
        }
    }
    let common = [
        "/opt/homebrew/bin/7zz",
        "/usr/local/bin/7zz",
        "/usr/bin/7zz",
        "/usr/bin/7z",
        "C:/Program Files/7-Zip/7z.exe",
    ];
    common
        .iter()
        .map(Path::new)
        .find(|p| p.is_file())
        .map(Path::to_path_buf)
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path_env = std::env::var_os("PATH")?;
    std::env::split_paths(&path_env)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}
