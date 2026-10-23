//! 金手指（cheat codes）支持。
//!
//! 所有格式最终归一化为统一内部模型 [`Cheat`]（地址 + 值 + 可选比较字节），
//! 由模拟器核心在 CPU 总线读取时拦截生效（与 Game Genie 硬件
//! 拦截 PRG 读取的语义一致，同时覆盖 RAM 地址）。
//!
//! 支持的输入格式：
//! - Game Genie 字母码（6/8 字符，如 `SXIOPO`）
//! - Pro Action Replay 十六进制码（8 字符，Nestopia 称 "Pro Action Rocky"）
//! - 原生码 `地址:值[:比较]`（FCEUX 风格）或 `地址-值[-比较]`（VirtuaNES 风格）
//! - FCEUX 文本 .cht 文件（[`parse_fceux_cht`]）
//! - Nestopia/Mesen XML .cht 文件（[`parse_xml_cht`]）
//!
//! [`parse_cht_file`] 按内容自动识别两种 .cht 文件。
//!
//! ```no_run
//! use nes_sim::cheat::parse_code;
//! use nes_sim::{CoreCommand, NES};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut nes = NES::new();
//! let rom = std::fs::read("game.nes")?;
//! nes.load_cartridge_ines(&rom)?;
//!
//! // "079F-01-01"：VirtuaNES 风格，往 $079F 写 01（仅当原值为 01）
//! let cheat = parse_code("079F-01-01")?;
//! nes.execute(CoreCommand::AddCheat(cheat));
//! # Ok(())
//! # }
//! ```

use core::fmt;
use core::str::FromStr;

/// Game Genie 字母表（恰好 16 个字符，顺序即 nibble 值 0x0–0xF）。
const GAME_GENIE_ALPHABET: &[u8; 16] = b"APZLGITYEOXUKSVN";

/// Pro Action Replay 位置换表（来自 Nestopia `ProActionRocky` 实现）。
const PAR_BIT_LUT: [u32; 31] = [
    3, 13, 14, 1, 6, 9, 5, 0, 12, 7, 2, 8, 10, 11, 4, 19, 21, 23, 22, 20, 17, 16, 18, 29, 31, 24,
    26, 25, 30, 27, 28,
];

const PAR_INITIAL_KEY: u32 = 0xFCBD_D274;
const PAR_POLYNOMIAL: u32 = 0xB830_9722;

/// 统一金手指模型：拦截 CPU 对 `address` 的读取。
///
/// - 无 `compare`：恒返回 `value`；
/// - 有 `compare`：仅当该地址的原始值等于 `compare` 时返回 `value`，
///   否则透传原值（防止游戏在同一地址复用不同数据时破坏状态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cheat {
    /// CPU 地址空间地址（$0000–$FFFF）。
    pub address: u16,
    /// 替换值。
    pub value: u8,
    /// 可选比较字节：仅当原始值等于它时才替换。
    pub compare: Option<u8>,
    /// 是否启用；禁用的金手指不参与总线拦截。
    pub enabled: bool,
    /// 描述文字（导入 .cht 文件时保留）。
    pub description: String,
    /// Nestopia/Mesen XML 中的 `game` 属性（CRC32），绑定特定游戏；
    /// 导入时保留，是否过滤由前端决定。
    pub game_crc: Option<u32>,
}

impl Cheat {
    /// 创建启用的无比较金手指。
    pub fn new(address: u16, value: u8) -> Self {
        Self {
            address,
            value,
            compare: None,
            enabled: true,
            description: String::new(),
            game_crc: None,
        }
    }

    /// 链式设置比较字节。
    pub fn with_compare(mut self, compare: u8) -> Self {
        self.compare = Some(compare);
        self
    }

    /// 链式设置描述。
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// 生成 Game Genie 显示码；地址必须在 $8000 以上。
    pub fn to_game_genie(&self) -> Result<String, CheatError> {
        encode_game_genie(self.address, self.value, self.compare)
    }

    /// 生成 Pro Action Replay 显示码；地址必须在 $8000 以上且带比较字节。
    pub fn to_pro_action_replay(&self) -> Result<String, CheatError> {
        self.compare
            .ok_or(CheatError::UnsupportedAddress)
            .and_then(|compare| encode_pro_action_replay(self.address, self.value, compare))
    }
}

impl FromStr for Cheat {
    type Err = CheatError;

    /// 等价于 [`parse_code`]：`"079F-01-01".parse::<Cheat>()`。
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_code(s)
    }
}

/// 金手指解析错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheatError {
    /// 输入为空或全为空白。
    Empty,
    /// 无法识别的代码格式。
    UnknownFormat,
    /// Game Genie 码非法（长度不是 6/8，或含字母表外字符）。
    InvalidGameGenie,
    /// Pro Action Replay 码非法（长度不是 8，或含非十六进制字符）。
    InvalidProActionReplay,
    /// 原生码字段非法。
    InvalidRaw,
    /// 地址无法用目标格式表示（如 Game Genie/PAR 要求 $8000 以上，PAR 要求比较字节）。
    UnsupportedAddress,
    /// XML .cht 结构非法。
    InvalidChtXml(&'static str),
}

impl fmt::Display for CheatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "empty cheat code"),
            Self::UnknownFormat => write!(f, "unrecognized cheat code format"),
            Self::InvalidGameGenie => {
                write!(
                    f,
                    "invalid Game Genie code (6/8 letters from A P Z L G I T Y E O X U K S V N)"
                )
            }
            Self::InvalidProActionReplay => {
                write!(f, "invalid Pro Action Replay code (8 hex digits)")
            }
            Self::InvalidRaw => write!(f, "invalid raw cheat fields"),
            Self::UnsupportedAddress => {
                write!(f, "address not representable in target code format")
            }
            Self::InvalidChtXml(reason) => write!(f, "invalid XML cheat file: {reason}"),
        }
    }
}

impl std::error::Error for CheatError {}

/// 解析单条金手指代码，按特征自动识别格式：
///
/// - 6/8 字符且全部属于 Game Genie 字母表 → Game Genie；
/// - 8 个十六进制字符 → Pro Action Replay
///   （注意：仅由 A/E 组成的 8 字符串会被优先识别为 Game Genie）；
/// - 含 `:` → 原生码 `地址:值[:比较][:描述]`；
/// - 含 `-` → 原生码 `地址-值[-比较][-描述]`（VirtuaNES 风格）。
pub fn parse_code(code: &str) -> Result<Cheat, CheatError> {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return Err(CheatError::Empty);
    }

    let upper = trimmed.to_ascii_uppercase();
    let bytes = upper.as_bytes();
    let is_game_genie =
        (bytes.len() == 6 || bytes.len() == 8) && bytes.iter().all(|&c| gg_nibble(c).is_some());
    if is_game_genie {
        return parse_game_genie(trimmed);
    }
    if bytes.len() == 8 && bytes.iter().all(u8::is_ascii_hexdigit) {
        return parse_pro_action_replay(trimmed);
    }
    if trimmed.contains(':') {
        return parse_raw(trimmed, ":");
    }
    if trimmed.contains('-') {
        return parse_raw(trimmed, "-");
    }
    Err(CheatError::UnknownFormat)
}

/// 解析 Game Genie 字母码（大小写不敏感）。
///
/// 6 字符码解码为地址+值；8 字符码额外携带比较字节。
/// 地址恒 >= $8000（Game Genie 硬件只能拦截 PRG 区）。
pub fn parse_game_genie(code: &str) -> Result<Cheat, CheatError> {
    let upper = code.trim().to_ascii_uppercase();
    let bytes = upper.as_bytes();
    if bytes.len() != 6 && bytes.len() != 8 {
        return Err(CheatError::InvalidGameGenie);
    }
    let mut n = [0u8; 8];
    for (i, &c) in bytes.iter().enumerate() {
        n[i] = gg_nibble(c).ok_or(CheatError::InvalidGameGenie)?;
    }

    let address = 0x8000
        | (n[4] as u16 & 0x7)
        | (n[3] as u16 & 0x8)
        | (n[2] as u16 & 0x7) << 4
        | (n[1] as u16 & 0x8) << 4
        | (n[5] as u16 & 0x7) << 8
        | (n[4] as u16 & 0x8) << 8
        | (n[3] as u16 & 0x7) << 12;
    let mut value = (n[0] & 0x7) | (n[1] & 0x7) << 4 | (n[0] & 0x8) << 4;

    let compare = if bytes.len() == 8 {
        value |= n[7] & 0x8;
        Some((n[6] & 0x7) | (n[5] & 0x8) | (n[7] & 0x7) << 4 | (n[6] & 0x8) << 4)
    } else {
        value |= n[5] & 0x8;
        None
    };

    Ok(Cheat::new(address, value).with_compare_opt(compare))
}

/// 生成 Game Genie 字母码；地址必须在 $8000 以上。
pub fn encode_game_genie(
    address: u16,
    value: u8,
    compare: Option<u8>,
) -> Result<String, CheatError> {
    if address < 0x8000 {
        return Err(CheatError::UnsupportedAddress);
    }
    let use_compare = compare.is_some();
    let compare_or_value = compare.unwrap_or(value);
    let codes = [
        (value & 0x7) | (value >> 4 & 0x8),
        (value >> 4 & 0x7) | (address >> 4 & 0x8) as u8,
        (address >> 4 & 0x7) as u8 | if use_compare { 0x8 } else { 0 },
        (address >> 12 & 0x7) as u8 | (address & 0x8) as u8,
        (address & 0x7) as u8 | (address >> 8 & 0x8) as u8,
        (address >> 8 & 0x7) as u8 | (compare_or_value & 0x8),
        if let Some(c) = compare {
            (c & 0x7) | (c >> 4 & 0x8)
        } else {
            0
        },
        if let Some(c) = compare {
            (c >> 4 & 0x7) | (value & 0x8)
        } else {
            0
        },
    ];

    let len = if use_compare { 8 } else { 6 };
    let mut out = String::with_capacity(len);
    for &code in &codes[..len] {
        out.push(GAME_GENIE_ALPHABET[code as usize] as char);
    }
    Ok(out)
}

/// 解析 Pro Action Replay 码（8 个十六进制字符，大小写不敏感）。
///
/// NES 版 PAR 码经位置换 + LFSR 式加密，解码后恒为
/// 地址（>= $8000）+ 值 + 比较字节（必带）。
pub fn parse_pro_action_replay(code: &str) -> Result<Cheat, CheatError> {
    let upper = code.trim().to_ascii_uppercase();
    if upper.len() != 8 || !upper.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(CheatError::InvalidProActionReplay);
    }
    // 输入字符串即密文的大端十六进制表示。
    let mut input = u32::from_str_radix(&upper, 16).expect("validated hex digits");

    let mut output: u32 = 0;
    let mut key = PAR_INITIAL_KEY;
    for i in (0..31).rev() {
        if (key ^ input) & 0x8000_0000 != 0 {
            output |= 1 << PAR_BIT_LUT[i];
            key ^= PAR_POLYNOMIAL;
        }
        input <<= 1;
        key <<= 1;
    }

    let address = 0x8000 | (output & 0x7FFF) as u16;
    let compare = (output >> 16) as u8;
    let value = (output >> 24) as u8;
    Ok(Cheat::new(address, value).with_compare(compare))
}

/// 生成 Pro Action Replay 码；地址必须在 $8000 以上且必须提供比较字节。
pub fn encode_pro_action_replay(
    address: u16,
    value: u8,
    compare: u8,
) -> Result<String, CheatError> {
    if address < 0x8000 {
        return Err(CheatError::UnsupportedAddress);
    }
    let input = (address & 0x7FFF) as u32 | (compare as u32) << 16 | (value as u32) << 24;

    let mut output: u32 = 0;
    let mut key = PAR_INITIAL_KEY;
    for i in (0..31).rev() {
        let ctrl = input >> PAR_BIT_LUT[i] & 1;
        output |= ((key >> 31) ^ ctrl) << (i + 1);
        if ctrl != 0 {
            key ^= PAR_POLYNOMIAL;
        }
        key <<= 1;
    }
    Ok(format!("{output:08X}"))
}

/// 解析 FCEUX 文本 .cht 文件。
///
/// 每行一条：`[S][C][:]地址:值[:比较]:描述`。
/// `S`（substitution）在本实现中无意义——所有金手指统一走读取拦截；
/// `C` 表示带比较字节；前缀 `:` 表示该条已禁用。
///
/// 与 FCEUX 一致采用宽松策略：无法解析的行直接跳过。
pub fn parse_fceux_cht(text: &str) -> Vec<Cheat> {
    text.lines().filter_map(parse_fceux_line).collect()
}

fn parse_fceux_line(line: &str) -> Option<Cheat> {
    let mut rest = line.trim();
    let mut expect_compare = false;
    let mut enabled = true;
    while let Some(first) = rest.chars().next() {
        match first {
            'S' | 's' => rest = &rest[1..],
            'C' | 'c' => {
                expect_compare = true;
                rest = &rest[1..];
            }
            ':' => {
                enabled = false;
                rest = &rest[1..];
            }
            _ => break,
        }
    }

    let mut fields = rest.split(':');
    let address = parse_hex_field(fields.next()?, 0xFFFF)? as u16;
    let value = parse_hex_field(fields.next()?, 0xFF)? as u8;
    let mut compare = None;
    let mut description_parts: Vec<&str> = Vec::new();

    if expect_compare {
        compare = Some(parse_hex_field(fields.next()?, 0xFF)? as u8);
        description_parts = fields.collect();
    } else {
        let third = fields.next();
        match third {
            // 无 C 标记但第三个字段恰好是 1–2 位十六进制：按比较字节处理。
            Some(field) if parse_hex_field(field, 0xFF).is_some() => {
                compare = parse_hex_field(field, 0xFF).map(|v| v as u8);
                description_parts = fields.collect();
            }
            _ => {
                if let Some(field) = third {
                    description_parts.push(field);
                }
                description_parts.extend(fields);
            }
        }
    }

    let mut cheat = Cheat::new(address, value).with_compare_opt(compare);
    cheat.enabled = enabled;
    cheat.description = description_parts.join(":").trim().to_string();
    Some(cheat)
}

/// 解析 Nestopia/Mesen XML .cht 文件。
///
/// 固定 schema：
/// ```xml
/// <cheats>
///   <cheat enabled="1" game="CRC32HEX">
///     <address>0x07FD</address>  <!-- 或 <genie>SXIOPO</genie> / <rocky>XXXXXXXX</rocky> -->
///     <value>99</value>
///     <compare>0</compare>
///     <description>描述</description>
///   </cheat>
/// </cheats>
/// ```
/// 数值接受十进制或 `0x`/`$` 前缀十六进制；无法解析的单条 `cheat` 元素跳过。
pub fn parse_xml_cht(text: &str) -> Result<Vec<Cheat>, CheatError> {
    let text = text.trim_start_matches('\u{FEFF}').trim();
    if !text.starts_with('<') {
        return Err(CheatError::InvalidChtXml("content is not XML"));
    }
    let text = strip_xml_noise(text);
    let root = find_element(&text, "cheats")
        .ok_or(CheatError::InvalidChtXml("missing <cheats> root element"))?
        .1
        .ok_or(CheatError::InvalidChtXml("<cheats> is self-closing"))?;
    let mut cheats = Vec::new();
    let mut rest = root;
    while let Some((attrs, inner)) = find_element(rest, "cheat") {
        // 自闭合 <cheat/> 无子元素，跳过。
        let inner = inner.unwrap_or("");
        if let Some(cheat) = parse_xml_cheat_element(attrs, inner) {
            cheats.push(cheat);
        }
        // 前进到当前元素结束之后。
        let end = find_element_end(rest, "cheat").expect("matched element");
        rest = &rest[end..];
    }
    Ok(cheats)
}

/// 剔除注释 `<!--...-->` 与处理指令/声明 `<?...?>`，
/// 避免它们出现在 cheat 元素内部时中断子元素扫描。
fn strip_xml_noise(text: &str) -> String {
    let without_comments = strip_delimited(text, "<!--", "-->");
    strip_delimited(&without_comments, "<?", "?>")
}

fn strip_delimited(text: &str, open: &str, close: &str) -> String {
    let mut cleaned = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(open) {
        cleaned.push_str(&rest[..start]);
        match rest[start..].find(close) {
            Some(end) => rest = &rest[start + end + close.len()..],
            // 未终止的块：丢弃尾部。
            None => return cleaned,
        }
    }
    cleaned.push_str(rest);
    cleaned
}

/// 按内容嗅探并解析 .cht 文件：以 `<` 开头按 XML 处理，否则按 FCEUX 文本处理。
pub fn parse_cht_file(text: &str) -> Result<Vec<Cheat>, CheatError> {
    if text
        .trim_start_matches('\u{FEFF}')
        .trim_start()
        .starts_with('<')
    {
        parse_xml_cht(text)
    } else {
        Ok(parse_fceux_cht(text))
    }
}

fn parse_xml_cheat_element(attrs: &str, inner: &str) -> Option<Cheat> {
    let mut cheat = None;

    let mut address: Option<u16> = None;
    let mut value: Option<u8> = None;
    let mut compare: Option<u8> = None;
    let mut description = String::new();

    let mut rest = inner;
    while let Some((tag, _attrs, content)) = find_child_element(rest) {
        match tag {
            "address" => {
                address = parse_num_u32(content)
                    .filter(|v| *v <= 0xFFFF)
                    .map(|v| v as u16)
            }
            "value" => {
                value = parse_num_u32(content)
                    .filter(|v| *v <= 0xFF)
                    .map(|v| v as u8)
            }
            "compare" => {
                compare = parse_num_u32(content)
                    .filter(|v| *v <= 0xFF)
                    .map(|v| v as u8)
            }
            "genie" => cheat = parse_game_genie(content).ok(),
            "rocky" => cheat = parse_pro_action_replay(content).ok(),
            "description" => description = decode_xml_entities(content.trim()),
            _ => {}
        }
        let end = find_child_element_end(rest).expect("matched child element");
        rest = &rest[end..];
    }

    let cheat = match address {
        // address/value/compare 优先（与 Nestopia 导入逻辑一致）。
        Some(address) => {
            let value = value?;
            Cheat::new(address, value).with_compare_opt(compare)
        }
        None => cheat?,
    };
    let enabled = match find_attribute(attrs, "enabled") {
        Some(v) => v != "0" && !v.eq_ignore_ascii_case("false"),
        None => true,
    };
    let game_crc = find_attribute(attrs, "game")
        .and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok());

    let mut cheat = cheat;
    cheat.enabled = enabled;
    cheat.game_crc = game_crc;
    cheat.description = description;
    Some(cheat)
}

/// 在 `text` 中查找 `<name ...>` 元素，返回 `(属性串, Some(内部内容))`；
/// 自闭合标签返回 `(属性串, None)`。
fn find_element<'a>(text: &'a str, name: &str) -> Option<(&'a str, Option<&'a str>)> {
    let open = format!("<{name}");
    let start = text.find(&open).filter(|&pos| {
        let after = text[pos + open.len()..].chars().next();
        matches!(
            after,
            Some(' ') | Some('\t') | Some('\n') | Some('\r') | Some('>')
        )
    })?;
    let tag_end = text[start..].find('>')? + start;
    let attrs = &text[start + open.len()..tag_end];
    let attrs = attrs.strip_suffix('/').unwrap_or(attrs);
    if text[..tag_end].ends_with('/') {
        return Some((attrs, None));
    }
    let close = format!("</{name}>");
    let content_end = text[tag_end + 1..].find(&close)? + tag_end + 1;
    Some((attrs, Some(&text[tag_end + 1..content_end])))
}

/// 返回 `find_element` 匹配元素（含闭合标签）之后的偏移。
fn find_element_end(text: &str, name: &str) -> Option<usize> {
    let open = format!("<{name}");
    let start = text.find(&open).filter(|&pos| {
        let after = text[pos + open.len()..].chars().next();
        matches!(
            after,
            Some(' ') | Some('\t') | Some('\n') | Some('\r') | Some('>')
        )
    })?;
    let tag_end = text[start..].find('>')? + start;
    if text[..tag_end].ends_with('/') {
        return Some(tag_end + 1);
    }
    let close = format!("</{name}>");
    let content_end = text[tag_end + 1..].find(&close)? + tag_end + 1;
    Some(content_end + close.len())
}

/// 扫描子元素：`(标签名, 属性串, 文本内容)`，未知标签同样返回以供跳过。
fn find_child_element(text: &str) -> Option<(&str, &str, &str)> {
    let start = text.find('<')?;
    let name_start = start + 1;
    let name_len = text[name_start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .count();
    if name_len == 0 {
        return None; // 注释、CDATA 等不支持
    }
    let name = &text[name_start..name_start + name_len];
    let (attrs, content) = find_element(&text[start..], name)?;
    Some((name, attrs, content.unwrap_or("")))
}

fn find_child_element_end(text: &str) -> Option<usize> {
    let start = text.find('<')?;
    let name_start = start + 1;
    let name_len = text[name_start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .count();
    if name_len == 0 {
        return None;
    }
    let name = &text[name_start..name_start + name_len];
    let end = find_element_end(&text[start..], name)?;
    Some(start + end)
}

fn find_attribute<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let pattern = format!("{name}=");
    let pos = attrs.find(&pattern)?;
    let rest = &attrs[pos + pattern.len()..];
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let value = &rest[1..];
    let end = value.find(quote)?;
    Some(&value[..end])
}

fn decode_xml_entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// 十进制，或 `0x`/`$` 前缀十六进制；无前缀时优先按十进制解析。
fn parse_num_u32(text: &str) -> Option<u32> {
    let t = text.trim();
    if let Some(hex) = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .or_else(|| t.strip_prefix('$'))
    {
        return u32::from_str_radix(hex, 16).ok();
    }
    t.parse::<u32>()
        .ok()
        .or_else(|| u32::from_str_radix(t, 16).ok())
}

fn parse_hex_field(field: &str, max: u32) -> Option<u32> {
    let value = u32::from_str_radix(field.trim(), 16).ok()?;
    (value <= max).then_some(value)
}

fn parse_raw(text: &str, separator: &str) -> Result<Cheat, CheatError> {
    let mut fields = text.split(separator);
    let address = fields
        .next()
        .and_then(|f| parse_hex_field(f, 0xFFFF))
        .ok_or(CheatError::InvalidRaw)? as u16;
    let value = fields
        .next()
        .and_then(|f| parse_hex_field(f, 0xFF))
        .ok_or(CheatError::InvalidRaw)? as u8;
    let mut compare = None;
    let mut description_parts: Vec<&str> = Vec::new();
    let third = fields.next();
    match third {
        Some(field) if parse_hex_field(field, 0xFF).is_some() => {
            compare = parse_hex_field(field, 0xFF).map(|v| v as u8);
            description_parts = fields.collect();
        }
        _ => {
            if let Some(field) = third {
                description_parts.push(field);
            }
            description_parts.extend(fields);
        }
    }

    let description = description_parts.join(separator).trim().to_string();
    Ok(Cheat::new(address, value)
        .with_compare_opt(compare)
        .with_description(description))
}

fn gg_nibble(c: u8) -> Option<u8> {
    GAME_GENIE_ALPHABET
        .iter()
        .position(|&l| l == c)
        .map(|p| p as u8)
}

impl Cheat {
    /// [`Cheat::with_compare`] 的 `Option` 版本（解析器内部用）。
    pub fn with_compare_opt(mut self, compare: Option<u8>) -> Self {
        self.compare = compare;
        self
    }
}

/// 总线读取拦截表：启用中的金手指按地址排序去重，供热路径二分查找。
#[derive(Clone, Debug, Default)]
pub(crate) struct CheatTable {
    entries: Vec<CheatEntry>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CheatEntry {
    address: u16,
    value: u8,
    compare: Option<u8>,
}

impl CheatTable {
    /// 由金手指主列表重建；同一地址存在多条时后添加的生效。
    pub(crate) fn rebuild(&mut self, cheats: &[Cheat]) {
        self.entries.clear();
        for cheat in cheats.iter().filter(|c| c.enabled) {
            let entry = CheatEntry {
                address: cheat.address,
                value: cheat.value,
                compare: cheat.compare,
            };
            match self
                .entries
                .binary_search_by_key(&entry.address, |e| e.address)
            {
                Ok(i) => self.entries[i] = entry,
                Err(i) => self.entries.insert(i, entry),
            }
        }
    }

    #[inline]
    pub(crate) fn is_active(&self) -> bool {
        !self.entries.is_empty()
    }

    /// 地址命中且（无比较或原始值匹配）时返回替换值。
    #[inline]
    pub(crate) fn substitute(&self, addr: u16, raw: u8) -> Option<u8> {
        let index = self.entries.partition_point(|e| e.address < addr);
        let entry = self.entries.get(index)?;
        if entry.address != addr {
            return None;
        }
        match entry.compare {
            Some(compare) if compare != raw => None,
            _ => Some(entry.value),
        }
    }
}

#[cfg(test)]
mod tests;
