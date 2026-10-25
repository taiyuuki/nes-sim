use crate::api::VideoFrame;

const NES_RGB_PALETTE: [[u8; 3]; 64] = [
    [84, 84, 84],
    [0, 30, 116],
    [8, 16, 144],
    [48, 0, 136],
    [68, 0, 100],
    [92, 0, 48],
    [84, 4, 0],
    [60, 24, 0],
    [32, 42, 0],
    [8, 58, 0],
    [0, 64, 0],
    [0, 60, 0],
    [0, 50, 60],
    [0, 0, 0],
    [0, 0, 0],
    [0, 0, 0],
    [152, 150, 152],
    [8, 76, 196],
    [48, 50, 236],
    [92, 30, 228],
    [136, 20, 176],
    [160, 20, 100],
    [152, 34, 32],
    [120, 60, 0],
    [84, 90, 0],
    [40, 114, 0],
    [8, 124, 0],
    [0, 118, 40],
    [0, 102, 120],
    [0, 0, 0],
    [0, 0, 0],
    [0, 0, 0],
    [236, 238, 236],
    [76, 154, 236],
    [120, 124, 236],
    [176, 98, 236],
    [228, 84, 236],
    [236, 88, 180],
    [236, 106, 100],
    [212, 136, 32],
    [160, 170, 0],
    [116, 196, 0],
    [76, 208, 32],
    [56, 204, 108],
    [56, 180, 204],
    [60, 60, 60],
    [0, 0, 0],
    [0, 0, 0],
    [236, 238, 236],
    [168, 204, 236],
    [188, 188, 236],
    [212, 178, 236],
    [236, 174, 236],
    [236, 174, 212],
    [236, 180, 176],
    [228, 196, 144],
    [204, 210, 120],
    [180, 222, 120],
    [168, 226, 144],
    [152, 226, 180],
    [160, 214, 228],
    [160, 162, 160],
    [0, 0, 0],
    [0, 0, 0],
];

pub fn palette_index_to_rgb(index: u8) -> [u8; 3] {
    Palette::DEFAULT.lookup(index, 0)
}

/// RGB 空间的颜色强调调制系数（千分比，实测拟合表，Nesdev NTSC video "Color Tint Bits"）。
/// 索引为 RGB 语义掩码（bit0=R, bit1=G, bit2=B），即 [`crate::api::VideoFrame::emphasis`]。
/// 语义：强调一色的同时压暗另外两色；三位全开时整体压暗。
const EMPHASIS_TINT: [[i32; 3]; 8] = [
    [1000, 1000, 1000], // 无强调
    [1239, 915, 743],   // R
    [794, 1086, 882],   // G
    [1019, 980, 653],   // RG (黄)
    [905, 1026, 1277],  // B
    [1023, 908, 979],   // RB (品红)
    [741, 987, 1001],   // GB (青)
    [750, 750, 750],    // RGB 全开（整体压暗）
];

/// 512 色调色板：64 色基色 × 8 种颜色强调组合。
/// 与 FCEUX/Nestopia 的 .pal 512 色文件同布局，可直接查表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// 扁平 RGB 三元组，索引 `(emphasis << 6 | color) * 3`。
    /// emphasis 为 RGB 语义掩码（bit0=R, bit1=G, bit2=B），与 .pal 文件块序一致。
    entries: [u8; 1536],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteError {
    /// .pal 长度既不是 192（64 色）也不是 1536（512 色）
    InvalidLength { len: usize },
}

impl std::fmt::Display for PaletteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaletteError::InvalidLength { len } => write!(
                f,
                "invalid .pal length {len}: expected 192 (64 colors) or 1536 (512 colors)"
            ),
        }
    }
}

impl std::error::Error for PaletteError {}

const fn tinted_byte(value: u8, scale: i32) -> u8 {
    let scaled = (value as i32 * scale) / 1000;
    if scaled > 255 {
        255
    } else if scaled < 0 {
        0
    } else {
        scaled as u8
    }
}

/// 默认调色板：内置 64 色表 + [`EMPHASIS_TINT`] 合成的强调块（编译期生成）。
const fn build_default_palette() -> Palette {
    let mut entries = [0u8; 1536];
    let mut emphasis = 0;
    while emphasis < 8 {
        let tint = EMPHASIS_TINT[emphasis];
        let mut color = 0;
        while color < 64 {
            let base = NES_RGB_PALETTE[color];
            let offset = ((emphasis << 6) | color) * 3;
            if emphasis == 0 || (color & 0x0F) >= 0x0E {
                // $xE/$xF 黑白列不染色
                entries[offset] = base[0];
                entries[offset + 1] = base[1];
                entries[offset + 2] = base[2];
            } else {
                entries[offset] = tinted_byte(base[0], tint[0]);
                entries[offset + 1] = tinted_byte(base[1], tint[1]);
                entries[offset + 2] = tinted_byte(base[2], tint[2]);
            }
            color += 1;
        }
        emphasis += 1;
    }
    Palette { entries }
}

impl Palette {
    pub const DEFAULT: Palette = build_default_palette();

    /// 解析 `.pal` 调色板文件（FCEUX/Nestopia 通用格式，无 header 的裸 RGB）：
    ///
    /// - 192 字节 = 64 色基色；颜色强调块按 [`EMPHASIS_TINT`] 合成
    ///   （`$xE`/`$xF` 黑白列不染色），与 FCEUX "classic" 生成方式一致
    /// - 1536 字节 = 512 色（8 个 64 色块，块序 0/R/G/RG/B/RB/GB/RGB），
    ///   强调色直接采用文件内容
    pub fn from_pal_bytes(bytes: &[u8]) -> Result<Self, PaletteError> {
        let mut entries = [0u8; 1536];
        const LEN_512_COLOR: usize = 1536;
        const LEN_64_COLOR: usize = 192;
        match bytes.len() {
            LEN_512_COLOR => entries.copy_from_slice(bytes),
            LEN_64_COLOR => {
                entries[..192].copy_from_slice(bytes);
                let mut emphasis = 1;
                while emphasis < 8 {
                    let tint = EMPHASIS_TINT[emphasis];
                    let mut color = 0;
                    while color < 64 {
                        let offset = ((emphasis << 6) | color) * 3;
                        if (color & 0x0F) >= 0x0E {
                            // $xE/$xF 黑白列不染色：强调块内保持基色
                            entries[offset] = bytes[color * 3];
                            entries[offset + 1] = bytes[color * 3 + 1];
                            entries[offset + 2] = bytes[color * 3 + 2];
                        } else {
                            entries[offset] = tinted_byte(bytes[color * 3], tint[0]);
                            entries[offset + 1] = tinted_byte(bytes[color * 3 + 1], tint[1]);
                            entries[offset + 2] = tinted_byte(bytes[color * 3 + 2], tint[2]);
                        }
                        color += 1;
                    }
                    emphasis += 1;
                }
            }
            len => return Err(PaletteError::InvalidLength { len }),
        }
        Ok(Self { entries })
    }

    /// 查色：`index` 低 6 位色号 + `emphasis` RGB 语义掩码（bit0=R bit1=G bit2=B）。
    pub fn lookup(&self, index: u8, emphasis: u8) -> [u8; 3] {
        let offset = (((emphasis & 7) as usize) << 6 | (index & 0x3F) as usize) * 3;
        [
            self.entries[offset],
            self.entries[offset + 1],
            self.entries[offset + 2],
        ]
    }

    /// 原始 512×3 字节 RGB（8 个强调块；基色块为前 192 字节），供前端构建 LUT。
    pub fn rgb_entries(&self) -> &[u8; 1536] {
        &self.entries
    }
}

/// 应用颜色强调后的 RGB（默认调色板）。
/// `$xE`/`$xF` 两列（黑/白）不受强调影响，其余列（含 $1D 黑）按系数调制。
pub fn palette_index_to_rgb_tinted(index: u8, emphasis: u8) -> [u8; 3] {
    Palette::DEFAULT.lookup(index, emphasis)
}

pub fn frame_to_rgb_with(frame: VideoFrame<'_>, palette: &Palette) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(frame.pixels.len() * 3);
    for &pixel in frame.pixels {
        rgb.extend_from_slice(&palette.lookup(pixel, frame.emphasis));
    }
    rgb
}

pub fn frame_to_rgb(frame: VideoFrame<'_>) -> Vec<u8> {
    frame_to_rgb_with(frame, &Palette::DEFAULT)
}

pub fn frame_to_argb32_with(frame: VideoFrame<'_>, palette: &Palette) -> Vec<u32> {
    let mut argb = Vec::with_capacity(frame.pixels.len());
    for &pixel in frame.pixels {
        let [r, g, b] = palette.lookup(pixel, frame.emphasis);
        argb.push((0xFF_u32 << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b));
    }
    argb
}

pub fn frame_to_argb32(frame: VideoFrame<'_>) -> Vec<u32> {
    frame_to_argb32_with(frame, &Palette::DEFAULT)
}

/// 预分配的视频缓冲区，避免每帧堆分配
pub struct VideoBuffer {
    buffer: Vec<u32>,
}

impl VideoBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            buffer: vec![0; capacity],
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u32] {
        &mut self.buffer
    }

    pub fn as_slice(&self) -> &[u32] {
        &self.buffer
    }
}

/// 将帧数据转换为 ARGB32 格式，写入预分配的缓冲区
pub fn frame_to_argb32_into_with(frame: VideoFrame<'_>, palette: &Palette, output: &mut [u32]) {
    for (dst, &pixel) in output.iter_mut().zip(frame.pixels) {
        let [r, g, b] = palette.lookup(pixel, frame.emphasis);
        *dst = (0xFF_u32 << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
    }
}

/// 将帧数据转换为 ARGB32 格式，写入预分配的缓冲区（默认调色板）
pub fn frame_to_argb32_into(frame: VideoFrame<'_>, output: &mut [u32]) {
    frame_to_argb32_into_with(frame, &Palette::DEFAULT, output);
}

/// 将帧数据转换为 RGBA 字节格式（用于 Tauri IPC）
pub fn frame_to_rgba_with(frame: VideoFrame<'_>, palette: &Palette) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(frame.pixels.len() * 4);
    for &pixel in frame.pixels {
        let [r, g, b] = palette.lookup(pixel, frame.emphasis);
        rgba.push(r);
        rgba.push(g);
        rgba.push(b);
        rgba.push(255);
    }
    rgba
}

/// 将帧数据转换为 RGBA 字节格式（默认调色板，用于 Tauri IPC）
pub fn frame_to_rgba(frame: VideoFrame<'_>) -> Vec<u8> {
    frame_to_rgba_with(frame, &Palette::DEFAULT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_lookup_matches_legacy_tint_helper() {
        // 旧 palette_index_to_rgb_tinted 的语义必须与 DEFAULT 查表完全一致
        for emphasis in 0..8u8 {
            for index in 0..64u8 {
                let [r, g, b] = NES_RGB_PALETTE[index as usize];
                let tint = EMPHASIS_TINT[emphasis as usize];
                let expected = if emphasis == 0 || (index & 0x0F) >= 0x0E {
                    [r, g, b]
                } else {
                    [
                        ((i32::from(r) * tint[0]) / 1000).clamp(0, 255) as u8,
                        ((i32::from(g) * tint[1]) / 1000).clamp(0, 255) as u8,
                        ((i32::from(b) * tint[2]) / 1000).clamp(0, 255) as u8,
                    ]
                };
                assert_eq!(Palette::DEFAULT.lookup(index, emphasis), expected);
                assert_eq!(palette_index_to_rgb_tinted(index, emphasis), expected);
            }
        }
    }

    #[test]
    fn parse_192_uses_base_colors_and_synthesizes_emphasis() {
        let mut file = [0u8; 192];
        for color in 0..64 {
            file[color * 3] = color as u8;
            file[color * 3 + 1] = (255 - color) as u8;
            file[color * 3 + 2] = 7;
        }
        let palette = Palette::from_pal_bytes(&file).expect("192-byte pal parses");

        // 基色块 = 文件内容
        assert_eq!(palette.lookup(0x21, 0), [0x21, 255 - 0x21, 7]);
        // R 强调块按千分比系数合成
        let [r, g, b] = palette.lookup(0x21, 1);
        assert_eq!(i32::from(r), (0x21 * 1239) / 1000);
        assert_eq!(i32::from(g), ((255 - 0x21) * 915) / 1000);
        assert_eq!(i32::from(b), (7 * 743) / 1000);
        // $xE/$xF 黑白列不染色：强调块内保持基色
        assert_eq!(palette.lookup(0x0E, 1), [0x0E, 255 - 0x0E, 7]);
        assert_eq!(palette.lookup(0x2F, 7), [0x2F, 255 - 0x2F, 7]);
    }

    #[test]
    fn parse_512_uses_file_emphasis_blocks_verbatim() {
        let mut file = [0u8; 1536];
        for emphasis in 0..8 {
            for color in 0..64 {
                let offset = ((emphasis << 6) | color) * 3;
                file[offset] = (emphasis * 10) as u8;
                file[offset + 1] = color as u8;
                file[offset + 2] = 0xAA;
            }
        }
        let palette = Palette::from_pal_bytes(&file).expect("512-byte pal parses");
        for emphasis in [0u8, 1, 4, 7] {
            for color in [0x00u8, 0x15, 0x3F] {
                assert_eq!(
                    palette.lookup(color, emphasis),
                    [emphasis * 10, color, 0xAA]
                );
            }
        }
        // 512 色文件自带强调数据：不再做 tint 合成（如 $x1D 在 RGB 全开块也被替换）
        assert_eq!(palette.rgb_entries()[..192], file[..192]);
    }

    #[test]
    fn parse_rejects_invalid_lengths() {
        for len in [0, 1, 100, 384, 1536 - 3, 1536 + 1] {
            let bytes = vec![0u8; len];
            assert_eq!(
                Palette::from_pal_bytes(&bytes),
                Err(PaletteError::InvalidLength { len })
            );
        }
    }

    #[test]
    fn lookup_masks_index_and_emphasis() {
        // 越界位被掩码
        let rgb = Palette::DEFAULT.lookup(0x40 | 0x21, 0x09);
        assert_eq!(rgb, Palette::DEFAULT.lookup(0x21, 0x01));
    }
}
