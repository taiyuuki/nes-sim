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

pub fn frame_to_rgb(frame: VideoFrame<'_>) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(frame.pixels.len() * 3);
    for &pixel in frame.pixels {
        rgb.extend_from_slice(&palette_index_to_rgb(pixel));
    }
    rgb
}

pub fn frame_to_argb32(frame: VideoFrame<'_>) -> Vec<u32> {
    let mut argb = Vec::with_capacity(frame.pixels.len());
    for &pixel in frame.pixels {
        let [r, g, b] = palette_index_to_rgb(pixel);
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
        let [r, g, b] = palette_index_to_rgb(pixel);
        *dst = (0xFF_u32 << 24) | (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b);
    }
}

/// 将帧数据转换为 RGBA 字节格式（用于 Tauri IPC）
pub fn frame_to_rgba(frame: VideoFrame<'_>) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(frame.pixels.len() * 4);
    for &pixel in frame.pixels {
        let [r, g, b] = palette_index_to_rgb(pixel);
        rgba.push(r);
        rgba.push(g);
        rgba.push(b);
        rgba.push(255);
    }
    rgba
}
