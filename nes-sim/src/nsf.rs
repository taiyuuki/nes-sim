//! NSF（NES Sound Format）解析与播放驱动状态。
//!
//! NSF 是「一段 6502 程序 + 调用约定」：播放器把音乐数据按 4KB bank
//! 装入 `$8000+`，先以 `A`=曲目号(0起)、`X`=制式(0=NTSC,1=PAL) 调用 INIT
//! 例程，之后按头部速度字（微秒）周期性调用 PLAY 例程。声音完全由
//! NSF 程序写 APU/扩展芯片寄存器产生。

use crate::cartridge::CartridgeError;
use crate::cartridge::TVSystem;

pub const NSF_HEADER_LEN: usize = 128;

/// INIT/PLAY 调用返回哨兵地址（RTS 弹出后 PC 落到这里即调用结束）。
/// 位于 $FFF0：向量区，任何合法 NSF 代码都不会在此执行。
pub(crate) const NSF_SENTINEL_ADDR: u16 = 0xFFF0;

/// NTSC 推荐播放间隔：16666µs ≈ 60.002Hz（速度字为0时的缺省）。
const DEFAULT_SPEED_NTSC_US: u16 = 16_666;
/// PAL 推荐播放间隔：20000µs = 50Hz。
const DEFAULT_SPEED_PAL_US: u16 = 20_000;

/// 判断ROM数据是否为NSF文件（`NESM\x1a`魔数）。
pub fn is_nsf_image(rom: &[u8]) -> bool {
    rom.len() >= NSF_HEADER_LEN && &rom[0..5] == b"NESM\x1a"
}

/// 面向前端的NSF元数据。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NsfInfo {
    /// 曲名（头部 $0E，32字节，ASCII/扩展编码，原样保留）
    pub name: String,
    /// 艺术家（$2E）
    pub artist: String,
    /// 版权/发行（$4E）
    pub copyright: String,
    /// 曲目总数（1起）
    pub track_count: u16,
    /// 头部指定的起始曲目（1起）
    pub start_track: u16,
    /// 当前曲目（1起）
    pub current_track: u16,
    /// 制式
    pub tv_system: TVSystem,
    /// 扩展音源位掩码：bit0=VRC6 bit1=VRC7 bit2=FDS bit3=MMC5 bit4=N163 bit5=5B
    pub chips: u8,
}

impl NsfInfo {
    /// 扩展音源名称列表（按位序）。
    pub fn chip_names(&self) -> Vec<&'static str> {
        chip_names(self.chips)
    }
}

pub fn chip_names(chips: u8) -> Vec<&'static str> {
    const NAMES: [&str; 6] = ["VRC6", "VRC7", "FDS", "MMC5", "N163", "5B"];
    NAMES
        .iter()
        .enumerate()
        .filter(|(i, _)| chips & (1 << i) != 0)
        .map(|(_, name)| *name)
        .collect()
}

/// 解析后的NSF头部（crate内部使用）。
#[derive(Clone, Debug)]
pub(crate) struct NsfHeader {
    /// 原始128字节头（文本字段解码用）
    raw: [u8; NSF_HEADER_LEN],
    pub total_songs: u16,
    pub start_song: u16,
    pub load_addr: u16,
    pub init_addr: u16,
    pub play_addr: u16,
    pub speed_ntsc_us: u16,
    pub speed_pal_us: u16,
    /// $7A PAL/NTSC 位：bit0=PAL曲 bit1=双制式
    pub pal_bits: u8,
    /// $7B 扩展音源位
    pub chip_flags: u8,
    /// $70-$77 初始bank值
    pub banks: [u8; 8],
    /// 头部之后的音乐数据
    pub data: Vec<u8>,
}

impl NsfHeader {
    /// 制式：bit0=PAL 且非双制式 → PAL，否则 NTSC。
    pub fn tv_system(&self) -> TVSystem {
        if self.pal_bits & 0x01 != 0 && self.pal_bits & 0x02 == 0 {
            TVSystem::PAL
        } else {
            TVSystem::NTSC
        }
    }

    /// 是否启用bank切换（任一bank字节非0）。
    pub fn banked(&self) -> bool {
        self.banks.iter().any(|&b| b != 0)
    }

    /// 当前制式下的PLAY调用间隔（微秒）。速度字为0取推荐缺省值。
    pub fn play_speed_us(&self, tv: TVSystem) -> u64 {
        match tv {
            TVSystem::NTSC | TVSystem::DENDY => {
                let v = self.speed_ntsc_us;
                u64::from(if v == 0 { DEFAULT_SPEED_NTSC_US } else { v })
            }
            TVSystem::PAL => {
                let v = self.speed_pal_us;
                u64::from(if v == 0 { DEFAULT_SPEED_PAL_US } else { v })
            }
        }
    }

    pub fn info(&self, current_track: u16) -> NsfInfo {
        NsfInfo {
            name: decode_text(&self.name_bytes()),
            artist: decode_text(&self.artist_bytes()),
            copyright: decode_text(&self.copyright_bytes()),
            track_count: self.total_songs,
            start_track: self.start_song,
            current_track,
            tv_system: self.tv_system(),
            chips: self.chip_flags & 0x3F,
        }
    }

    fn name_bytes(&self) -> [u8; 32] {
        self.raw[0x0E..0x2E].try_into().unwrap()
    }
    fn artist_bytes(&self) -> [u8; 32] {
        self.raw[0x2E..0x4E].try_into().unwrap()
    }
    fn copyright_bytes(&self) -> [u8; 32] {
        self.raw[0x4E..0x6E].try_into().unwrap()
    }
}

/// 头部文本字段 → String：空格/零截断；非UTF-8按字节无损替换（日文
/// 曲名常见Shift-JIS/CP1252编码，编码判断交给前端展示层）。
fn decode_text(bytes: &[u8; 32]) -> String {
    let mut end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    while end > 0 && bytes[end - 1] == b' ' {
        end -= 1;
    }
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// 解析NSF文件（含128字节头校验）。
pub(crate) fn parse_nsf(rom: &[u8]) -> Result<NsfHeader, CartridgeError> {
    if rom.len() < NSF_HEADER_LEN {
        return Err(CartridgeError::FileTooSmall);
    }
    if !is_nsf_image(rom) {
        return Err(CartridgeError::InvalidNsf);
    }
    let raw: [u8; NSF_HEADER_LEN] = rom[..NSF_HEADER_LEN].try_into().unwrap();
    let version = raw[5];
    if version != 1 && version != 2 {
        return Err(CartridgeError::InvalidNsf);
    }
    let total_songs = raw[6].max(1) as u16;
    let start_song = (raw[7] as u16).clamp(1, total_songs);
    let load_addr = u16::from_le_bytes([raw[8], raw[9]]);
    let init_addr = u16::from_le_bytes([raw[10], raw[11]]);
    let play_addr = u16::from_le_bytes([raw[12], raw[13]]);
    // 装载地址限定在 $6000-$FFFF（更低地址的FDS rip依赖bank切换）
    if !(0x6000..=0xFFFF).contains(&load_addr) {
        return Err(CartridgeError::InvalidNsf);
    }
    Ok(NsfHeader {
        total_songs,
        start_song,
        load_addr,
        init_addr,
        play_addr,
        speed_ntsc_us: u16::from_le_bytes([raw[0x6E], raw[0x6F]]),
        speed_pal_us: u16::from_le_bytes([raw[0x78], raw[0x79]]),
        pal_bits: raw[0x7A],
        chip_flags: raw[0x7B],
        banks: raw[0x70..0x78].try_into().unwrap(),
        data: rom[NSF_HEADER_LEN..].to_vec(),
        raw,
    })
}

/// NSF播放驱动状态（挂在NES上，不参与savestate的芯片部分）。
#[derive(Clone, Debug)]
pub(crate) struct NsfDriver {
    pub info: NsfInfo,
    pub init_addr: u16,
    pub play_addr: u16,
    /// 当前曲目（1起）
    pub track: u16,
    /// PLAY调用间隔（CPU周期数）
    pub play_period_cycles: u64,
    /// 自由运行计时累加器（CPU周期）
    pub play_acc: i64,
    /// 上次对账时的CPU周期数
    pub last_service: u64,
    /// 非返回式PLAY进行中（预算耗尽仍未RTS）
    pub play_running: bool,
}

/// 微秒间隔 → CPU周期数。
pub(crate) fn speed_us_to_cycles(us: u64, cpu_hz: u64) -> u64 {
    (us * cpu_hz + 500_000) / 1_000_000
}
