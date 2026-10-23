//! FM2 电影（TAS 录像）支持。
//!
//! FM2 是 FCEUX 的纯文本录像格式（`refs/fceux/documentation/fm2.txt`），
//! 也是事实上的 NES 录像互操作标准（FCEUX/Mesen/TASVideos 通用）。
//!
//! 结构：头部键值对 + 输入日志（每帧一行 `|c|port0|port1|port2|`）。
//! 手柄字段 8 个字符按 bit7→bit0 对应 R L D U T S B A——与 NES 手柄
//! 移位寄存器顺序一致，因此解析出的字节可直接作为
//! [`ControllerState::from_bits`] 的输入。
//!
//! ```no_run
//! use nes_sim::movie::parse_fm2;
//! use nes_sim::{CoreCommand, NES};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut nes = NES::new();
//! let rom = std::fs::read("game.nes")?;
//! nes.load_cartridge_ines(&rom)?;
//!
//! let bytes = std::fs::read("movie.fm2")?;
//! let movie = parse_fm2(&bytes)?;
//! nes.execute(CoreCommand::PlayMovie(movie));
//! // 每帧 run_frame 自动套用录像输入，播完发出 CoreEvent::MovieFinished
//! # Ok(())
//! # }
//! ```

use core::fmt;
use core::str;

use crate::input::ControllerState;

/// FM2 输入日志中手柄字段的列助记（bit7→bit0）。
const GAMEPAD_MNEMONICS: [u8; 8] = *b"RLDUTSBA";

/// FM2 端口设备类型（port0–port2 键的取值）。
pub mod port_kind {
    /// 未连接。
    pub const NONE: u8 = 0;
    /// 标准手柄。
    pub const GAMEPAD: u8 = 1;
    /// 光枪（本实现不支持回放）。
    pub const ZAPPER: u8 = 2;
}

/// 单帧输入记录。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MovieFrame {
    /// 命令位域；bit0 = 该帧开头按下 Reset。
    pub commands: u32,
    /// port0/port1 的手柄状态；端口非手柄时无意义（保持全松开）。
    /// fourscore 录像取前两手柄。
    pub controllers: [ControllerState; 2],
}

impl MovieFrame {
    /// 该帧开头是否按下 Reset。
    pub fn reset(&self) -> bool {
        self.commands & 1 != 0
    }
}

/// 一部 FM2 电影：头部元数据 + 逐帧输入日志。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Movie {
    /// 格式版本，FM2 恒为 3。
    pub version: u32,
    /// 录制所用模拟器版本号（0 表示未知）。
    pub emu_version: u32,
    pub rerecord_count: u32,
    /// PAL 制式标志（NTSC 为 false）。
    pub pal_flag: bool,
    /// 录制时使用的 ROM 文件名。
    pub rom_filename: String,
    /// ROM 的 MD5（十六进制字符串；核心不计算也不校验，仅透传）。
    pub rom_checksum: String,
    /// 电影 GUID（仅透传）。
    pub guid: String,
    /// 四人手柄（fourscore）录像；本实现只回放前两手柄。
    pub fourscore: bool,
    /// 麦克风标志（仅透传）。
    pub microphone: bool,
    /// port0/port1/port2 的设备类型，见 [`port_kind`]。
    pub ports: [u8; 3],
    /// 头部注释（`comment` 键，可多条）。
    pub comments: Vec<String>,
    /// 字幕（`subtitle` 键，可多条；格式约定 "帧号 文本"，由前端解释）。
    pub subtitles: Vec<String>,
    /// 录像从 savestate 开始（`savestate` 键存在；本实现无法回放此类录像，
    /// 仅保留标记与原始字符串以便转存）。
    pub starts_from_savestate: bool,
    /// `savestate`/`saveram` 键的原始值（hex/base64 文本，仅透传）。
    pub savestate_blob: String,
    pub saveram_blob: String,
    /// 输入日志；长度即电影帧数。
    pub frames: Vec<MovieFrame>,
}

/// FM2 解析错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MovieError {
    /// 不是 FM2 文件（必须以 "version 3" 开头）。
    NotAnFm2,
    /// `version` 键的值不是 3。
    UnsupportedVersion(u32),
    /// 端口设备类型无法回放（如 Zapper），携带 port0/port1 键的值。
    UnsupportedPort(u8),
    /// 输入行格式非法，携带 1 起始行号。
    BadInputLine(usize),
    /// 二进制输入日志长度不是记录大小的整数倍。
    TruncatedBinaryLog,
}

impl fmt::Display for MovieError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnFm2 => write!(f, "not an FM2 movie (must start with \"version 3\")"),
            Self::UnsupportedVersion(v) => write!(f, "unsupported FM2 version {v}"),
            Self::UnsupportedPort(p) => write!(f, "unsupported port device type {p}"),
            Self::BadInputLine(line) => write!(f, "malformed input log line {line}"),
            Self::TruncatedBinaryLog => write!(f, "binary input log has trailing bytes"),
        }
    }
}

impl std::error::Error for MovieError {}

/// 解析 FM2 电影（文本或 `binary 1` 二进制变体）。
///
/// 与 FCEUX 一致的宽松策略：未识别的头部键被忽略。
/// Zapper 端口（port0/port1 = 2）返回 [`MovieError::UnsupportedPort`]。
pub fn parse_fm2(bytes: &[u8]) -> Result<Movie, MovieError> {
    if !bytes.starts_with(b"version 3") {
        return Err(MovieError::NotAnFm2);
    }

    let mut movie = Movie {
        version: 3,
        ports: [port_kind::GAMEPAD, port_kind::GAMEPAD, port_kind::NONE],
        ..Movie::default()
    };
    // 头部最长记录长度键（fm3 用：输入日志截断到 N 条）。
    let mut length_limit: Option<usize> = None;
    let mut binary = false;

    let mut offset = 0usize;
    let mut line_no = 0usize;
    while offset < bytes.len() {
        // 定位行尾（\n 或 \r\n）。
        let rest = &bytes[offset..];
        let line_end = rest.iter().position(|&b| b == b'\n').unwrap_or(rest.len());
        let mut line = &rest[..line_end];
        if line.last() == Some(&b'\r') {
            line = &line[..line.len() - 1];
        }
        line_no += 1;

        if line.first() == Some(&b'|') {
            if binary {
                // 该 '|' 之后全部是二进制记录。
                let data = &rest[1..];
                let (fourscore, ports) = (movie.fourscore, movie.ports);
                parse_binary_log(
                    data,
                    fourscore,
                    ports,
                    length_limit,
                    &mut movie.frames,
                    line_no,
                )?;
                return Ok(movie);
            }
            let (fourscore, ports) = (movie.fourscore, movie.ports);
            parse_input_line(line, fourscore, ports, line_no, &mut movie.frames)?;
        } else if !line.is_empty() {
            let (key, value) = split_key_value(line);
            install_header_value(&mut movie, &key, &value, &mut binary, &mut length_limit)?;
        }

        if line_end == rest.len() {
            break;
        }
        offset += line_end + 1;
    }

    if let Some(limit) = length_limit {
        movie.frames.truncate(limit);
    }
    Ok(movie)
}

/// 序列化为 FM2 文本格式（FCEUX 头部键顺序，输入日志恒用文本手柄助记）。
pub fn write_fm2(movie: &Movie) -> String {
    let mut out = String::with_capacity(128 + movie.frames.len() * 16);
    out.push_str("version 3\n");
    out.push_str(&format!("emuVersion {}\n", movie.emu_version));
    out.push_str(&format!("rerecordCount {}\n", movie.rerecord_count));
    out.push_str(&format!("palFlag {}\n", movie.pal_flag as u8));
    out.push_str(&format!("romFilename {}\n", movie.rom_filename));
    if !movie.rom_checksum.is_empty() {
        out.push_str(&format!("romChecksum {}\n", movie.rom_checksum));
    }
    if !movie.guid.is_empty() {
        out.push_str(&format!("guid {}\n", movie.guid));
    }
    out.push_str(&format!("fourscore {}\n", movie.fourscore as u8));
    out.push_str(&format!("microphone {}\n", movie.microphone as u8));
    out.push_str(&format!("port0 {}\n", movie.ports[0]));
    out.push_str(&format!("port1 {}\n", movie.ports[1]));
    out.push_str(&format!("port2 {}\n", movie.ports[2]));
    for comment in &movie.comments {
        out.push_str(&format!("comment {comment}\n"));
    }
    for subtitle in &movie.subtitles {
        out.push_str(&format!("subtitle {subtitle}\n"));
    }
    if movie.starts_from_savestate {
        if !movie.savestate_blob.is_empty() {
            out.push_str(&format!("savestate {}\n", movie.savestate_blob));
        }
        if !movie.saveram_blob.is_empty() {
            out.push_str(&format!("saveram {}\n", movie.saveram_blob));
        }
    }

    for frame in &movie.frames {
        out.push('|');
        out.push_str(&frame.commands.to_string());
        if movie.fourscore {
            for pad in 0..4 {
                out.push('|');
                // 只有前两手柄有数据，三/四号写全松开。
                let state = if pad < 2 {
                    frame.controllers[pad]
                } else {
                    ControllerState::new()
                };
                write_gamepad(&mut out, state);
            }
            // port2 空字段 + 结尾管道。
            out.push('|');
            out.push('|');
            out.push('\n');
        } else {
            for port in 0..2 {
                out.push('|');
                if movie.ports[port] == port_kind::GAMEPAD {
                    write_gamepad(&mut out, frame.controllers[port]);
                }
                // 其他端口类型（None 等）写字段留空。
            }
            // port2 字段（SIFC_NONE 为空）+ 结尾管道。
            out.push('|');
            if movie.ports[2] == port_kind::GAMEPAD {
                write_gamepad(&mut out, ControllerState::new());
            }
            out.push('|');
            out.push('\n');
        }
    }
    out
}

impl Movie {
    /// 序列化为 FM2 文本，等价于 [`write_fm2`]。
    pub fn to_fm2_string(&self) -> String {
        write_fm2(self)
    }
}

/// 解析一条输入日志行 `|c|port0|port1|port2|`（fourscore 为 4 手柄）。
fn parse_input_line(
    line: &[u8],
    fourscore: bool,
    ports: [u8; 3],
    line_no: usize,
    frames: &mut Vec<MovieFrame>,
) -> Result<(), MovieError> {
    let text = str::from_utf8(line).map_err(|_| MovieError::BadInputLine(line_no))?;
    let fields: Vec<&str> = text.split('|').collect();
    // "|c|p0|p1||" → ["", "c", "p0", "p1", "", ""]（末尾 port2 空字段，
    // 允许缺少结尾管道的变体）；fourscore 多两个手柄字段。
    let expected = if fourscore { 8 } else { 6 };
    if fields.len() != expected && fields.len() != expected - 1 {
        return Err(MovieError::BadInputLine(line_no));
    }

    let commands: u32 = fields[1]
        .trim()
        .parse()
        .map_err(|_| MovieError::BadInputLine(line_no))?;

    let mut frame = MovieFrame {
        commands,
        ..MovieFrame::default()
    };
    if fourscore {
        frame.controllers[0] = parse_gamepad(fields[2], line_no)?;
        frame.controllers[1] = parse_gamepad(fields[3], line_no)?;
        // 后两手柄（三/四号）不回放，仅跳过字段 4/5。
    } else {
        for port in 0..2 {
            match ports[port] {
                port_kind::GAMEPAD => {
                    frame.controllers[port] = parse_gamepad(fields[2 + port], line_no)?;
                }
                port_kind::NONE => {}
                other => return Err(MovieError::UnsupportedPort(other)),
            }
        }
    }
    frames.push(frame);
    Ok(())
}

/// 手柄字段：8 个字符，'.'/' ' 为松开，其余任意字符为按下。
/// 列顺序 RLDUTSBA（bit7→bit0），恰好等于 [`ControllerState::from_bits`] 布局。
fn parse_gamepad(field: &str, line_no: usize) -> Result<ControllerState, MovieError> {
    let bytes = field.as_bytes();
    if bytes.len() != 8 {
        return Err(MovieError::BadInputLine(line_no));
    }
    let mut bits = 0u8;
    for &b in bytes {
        bits <<= 1;
        if b != b'.' && b != b' ' {
            bits |= 1;
        }
    }
    Ok(ControllerState::from_bits(bits))
}

fn write_gamepad(out: &mut String, state: ControllerState) {
    let bits = state.bits();
    for i in (0..8).rev() {
        if bits & (1 << i) != 0 {
            out.push(GAMEPAD_MNEMONICS[7 - i] as char);
        } else {
            out.push('.');
        }
    }
}

/// 解析 `binary 1` 变体的紧凑记录：
/// 每帧 = 1 字节命令 + 每个手柄端口 1 字节（fourscore 为 4 字节）。
fn parse_binary_log(
    data: &[u8],
    fourscore: bool,
    ports: [u8; 3],
    length_limit: Option<usize>,
    frames: &mut Vec<MovieFrame>,
    line_no: usize,
) -> Result<(), MovieError> {
    let pads = if fourscore {
        4
    } else {
        ports
            .iter()
            .take(2)
            .filter(|&&p| p == port_kind::GAMEPAD)
            .count()
    };
    let record_size = 1 + pads;
    if !data.len().is_multiple_of(record_size) {
        return Err(MovieError::TruncatedBinaryLog);
    }
    let count = length_limit.map_or(data.len() / record_size, |limit| {
        limit.min(data.len() / record_size)
    });

    let mut offset = 0usize;
    for _ in 0..count {
        let commands = data[offset] as u32;
        offset += 1;
        let mut frame = MovieFrame {
            commands,
            ..MovieFrame::default()
        };
        if fourscore {
            // 前两手柄回放，后两手柄跳过。
            frame.controllers[0] = ControllerState::from_bits(data[offset]);
            frame.controllers[1] = ControllerState::from_bits(data[offset + 1]);
            offset += 4;
        } else {
            for (port, &kind) in ports.iter().enumerate().take(2) {
                if kind == port_kind::GAMEPAD {
                    frame.controllers[port] = ControllerState::from_bits(data[offset]);
                    offset += 1;
                }
            }
        }
        let _ = line_no;
        frames.push(frame);
    }
    Ok(())
}

/// 头部键值行：key 与 value 以首个空白分隔，value 为行内余下全部内容。
fn split_key_value(line: &[u8]) -> (String, String) {
    let text = String::from_utf8_lossy(line);
    let text = text.trim_end();
    match text.find([' ', '\t']) {
        Some(pos) => (
            text[..pos].to_string(),
            text[pos + 1..].trim_start().to_string(),
        ),
        None => (text.to_string(), String::new()),
    }
}

fn install_header_value(
    movie: &mut Movie,
    key: &str,
    value: &str,
    binary: &mut bool,
    length_limit: &mut Option<usize>,
) -> Result<(), MovieError> {
    match key {
        "version" => {
            let version: u32 = value.parse().map_err(|_| MovieError::NotAnFm2)?;
            if version != 3 {
                return Err(MovieError::UnsupportedVersion(version));
            }
        }
        "emuVersion" => movie.emu_version = value.parse().unwrap_or(0),
        "rerecordCount" => movie.rerecord_count = value.parse().unwrap_or(0),
        "palFlag" => movie.pal_flag = value != "0",
        "romFilename" => movie.rom_filename = value.to_string(),
        "romChecksum" => movie.rom_checksum = value.to_string(),
        "guid" => movie.guid = value.to_string(),
        "fourscore" => movie.fourscore = value != "0",
        "microphone" => movie.microphone = value != "0",
        "port0" => movie.ports[0] = parse_port(value)?,
        "port1" => movie.ports[1] = parse_port(value)?,
        "port2" => movie.ports[2] = parse_port(value).unwrap_or(port_kind::NONE),
        "comment" => movie.comments.push(value.to_string()),
        "subtitle" => movie.subtitles.push(value.to_string()),
        "binary" => *binary = value != "0",
        "length" => *length_limit = value.parse().ok(),
        "savestate" => {
            movie.starts_from_savestate = true;
            movie.savestate_blob = value.to_string();
        }
        "saveram" => movie.saveram_blob = value.to_string(),
        // FDS / NewPPU / RAMInitOption / RAMInitSeed 等同步相关键：
        // 本模拟器无对应概念，忽略（见模块文档）。
        _ => {}
    }
    Ok(())
}

fn parse_port(value: &str) -> Result<u8, MovieError> {
    match value.trim() {
        "0" => Ok(port_kind::NONE),
        "1" => Ok(port_kind::GAMEPAD),
        "2" => Err(MovieError::UnsupportedPort(port_kind::ZAPPER)),
        other => Err(MovieError::UnsupportedPort(other.parse().unwrap_or(255))),
    }
}

#[cfg(test)]
mod tests;
