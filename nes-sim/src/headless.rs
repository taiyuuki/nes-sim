use crate::api::VideoFrame;
use crate::video::palette_index_to_rgb;
use std::fs;
use std::io;
use std::path::Path;

pub fn frame_to_ppm(frame: VideoFrame<'_>) -> Vec<u8> {
    let mut ppm = Vec::with_capacity(16 + frame.pixels.len() * 3);
    ppm.extend_from_slice(format!("P6\n{} {}\n255\n", frame.width, frame.height).as_bytes());
    for &pixel in frame.pixels {
        ppm.extend_from_slice(&palette_index_to_rgb(pixel));
    }
    ppm
}

pub fn write_frame_ppm<P: AsRef<Path>>(path: P, frame: VideoFrame<'_>) -> io::Result<()> {
    fs::write(path, frame_to_ppm(frame))
}

pub fn stable_byte_hash(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
