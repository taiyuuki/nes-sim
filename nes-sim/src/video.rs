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
    NES_RGB_PALETTE[(index & 0x3F) as usize]
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

/// 应用颜色强调后的 RGB。
/// `$xE`/`$xF` 两列（黑/白）不受强调影响，其余列（含 $1D 黑）按系数调制。
pub fn palette_index_to_rgb_tinted(index: u8, emphasis: u8) -> [u8; 3] {
    let [r, g, b] = palette_index_to_rgb(index);
    let tint = EMPHASIS_TINT[(emphasis & 7) as usize];
    let low = index & 0x0F;
    if tint == [1000, 1000, 1000] || low >= 0x0E {
        return [r, g, b];
    }
    [
        ((i32::from(r) * tint[0]) / 1000).clamp(0, 255) as u8,
        ((i32::from(g) * tint[1]) / 1000).clamp(0, 255) as u8,
        ((i32::from(b) * tint[2]) / 1000).clamp(0, 255) as u8,
    ]
}

pub fn frame_to_rgb(frame: VideoFrame<'_>) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(frame.pixels.len() * 3);
    for &pixel in frame.pixels {
        rgb.extend_from_slice(&palette_index_to_rgb_tinted(pixel, frame.emphasis));
    }
    rgb
}

pub fn frame_to_argb32(frame: VideoFrame<'_>) -> Vec<u32> {
    let mut argb = Vec::with_capacity(frame.pixels.len());
    for &pixel in frame.pixels {
        let [r, g, b] = palette_index_to_rgb_tinted(pixel, frame.emphasis);
        argb.push((0xFF_u32 << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b));
    }
    argb
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
pub fn frame_to_argb32_into(frame: VideoFrame<'_>, output: &mut [u32]) {
    for (dst, &pixel) in output.iter_mut().zip(frame.pixels) {
        let [r, g, b] = palette_index_to_rgb_tinted(pixel, frame.emphasis);
        *dst = (0xFF_u32 << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
    }
}

/// 将帧数据转换为 RGBA 字节格式（用于 Tauri IPC）
pub fn frame_to_rgba(frame: VideoFrame<'_>) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(frame.pixels.len() * 4);
    for &pixel in frame.pixels {
        let [r, g, b] = palette_index_to_rgb_tinted(pixel, frame.emphasis);
        rgba.push(r);
        rgba.push(g);
        rgba.push(b);
        rgba.push(255);
    }
    rgba
}
