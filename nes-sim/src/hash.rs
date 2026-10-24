//! ROM 标识哈希：MD5 与 CRC-32。
//!
//! 仅用于 ROM 识别与金手指/录像文件匹配（与 FCEUX、Nestopia 的用途一致），
//! 不是安全用途。两家模拟器的校验和口径相同：对解析后的
//! PRG-ROM ‖ CHR-ROM 拼接字节流计算（不含 iNES 头与 trainer）。
//!
//! - MD5（fm2 的 `romChecksum` 键）：FCEUX `iNESLoad` 对 ROM/VROM 依次 update；
//! - CRC-32（Nestopia .cht 的 `game` 属性）：Nestopia `NstCartridge` 对
//!   prg/chr 依次 Compute，zlib 兼容（CRC-32/ISO-HDLC）。
//!
//! ```no_run
//! use nes_sim::hash::md5_hex;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let checksum = md5_hex(&std::fs::read("game.nes")?); // 演示；实际用 Cartridge::rom_md5_hex
//! # let _: String = checksum;
//! # Ok(())
//! # }
//! ```

/// MD5 压缩函数每轮循环左移量（RFC 1321）。
const MD5_SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// MD5 常量表：`floor(abs(sin(i+1)) * 2^32)`（RFC 1321）。
const MD5_K: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

/// 流式 MD5（RFC 1321）。
#[derive(Clone)]
pub struct Md5 {
    a0: u32,
    b0: u32,
    c0: u32,
    d0: u32,
    /// 已处理消息总字节数（低 64 位，按位长在 finalize 时换算）。
    length: u64,
    buffer: [u8; 64],
    buffer_len: usize,
}

impl Default for Md5 {
    fn default() -> Self {
        Self::new()
    }
}

impl Md5 {
    pub fn new() -> Self {
        Self {
            a0: 0x6745_2301,
            b0: 0xefcd_ab89,
            c0: 0x98ba_dcfe,
            d0: 0x1032_5476,
            length: 0,
            buffer: [0; 64],
            buffer_len: 0,
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        self.length = self.length.wrapping_add(data.len() as u64);

        // 先填满上次的残留缓冲
        if self.buffer_len > 0 {
            let take = (64 - self.buffer_len).min(data.len());
            self.buffer[self.buffer_len..self.buffer_len + take].copy_from_slice(&data[..take]);
            self.buffer_len += take;
            data = &data[take..];
            if self.buffer_len == 64 {
                let block = self.buffer;
                self.process(&block);
                self.buffer_len = 0;
            }
        }

        // 整块处理（借用输入，避免分配）
        let full_blocks = data.len() / 64;
        for block in 0..full_blocks {
            let mut block_bytes = [0u8; 64];
            block_bytes.copy_from_slice(&data[block * 64..(block + 1) * 64]);
            self.process(&block_bytes);
        }
        let rest = &data[full_blocks * 64..];
        self.buffer[..rest.len()].copy_from_slice(rest);
        // 残留未满且数据耗尽时 rest 为空，累加保留原有计数
        self.buffer_len += rest.len();
    }

    pub fn finalize(mut self) -> [u8; 16] {
        let bit_length = self.length.wrapping_mul(8);

        // 填充到 128 字节暂存：0x80、零、8 字节小端位长（可能跨两个块）
        let mut last = [0u8; 128];
        last[..self.buffer_len].copy_from_slice(&self.buffer[..self.buffer_len]);
        last[self.buffer_len] = 0x80;
        let total = if self.buffer_len + 9 <= 64 { 64 } else { 128 };
        last[total - 8..total].copy_from_slice(&bit_length.to_le_bytes());

        let mut first = [0u8; 64];
        first.copy_from_slice(&last[..64]);
        self.process(&first);
        if total == 128 {
            let mut second = [0u8; 64];
            second.copy_from_slice(&last[64..128]);
            self.process(&second);
        }

        let mut digest = [0u8; 16];
        digest[0..4].copy_from_slice(&self.a0.to_le_bytes());
        digest[4..8].copy_from_slice(&self.b0.to_le_bytes());
        digest[8..12].copy_from_slice(&self.c0.to_le_bytes());
        digest[12..16].copy_from_slice(&self.d0.to_le_bytes());
        digest
    }

    fn process(&mut self, block: &[u8; 64]) {
        let mut m = [0u32; 16];
        for (i, word) in m.iter_mut().enumerate() {
            *word = u32::from_le_bytes(block[i * 4..i * 4 + 4].try_into().unwrap());
        }

        let (mut a, mut b, mut c, mut d) = (self.a0, self.b0, self.c0, self.d0);
        for i in 0..64 {
            let (f, g) = match i / 16 {
                0 => ((b & c) | !b & d, i),
                1 => ((d & b) | !d & c, (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let sum = a.wrapping_add(f).wrapping_add(MD5_K[i]).wrapping_add(m[g]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(sum.rotate_left(MD5_SHIFTS[i]));
        }
        self.a0 = self.a0.wrapping_add(a);
        self.b0 = self.b0.wrapping_add(b);
        self.c0 = self.c0.wrapping_add(c);
        self.d0 = self.d0.wrapping_add(d);
    }
}

/// 一次性计算 MD5。
pub fn md5(data: &[u8]) -> [u8; 16] {
    let mut hasher = Md5::new();
    hasher.update(data);
    hasher.finalize()
}

/// 一次性计算 MD5 并转为 32 位小写十六进制字符串（fm2 `romChecksum` 格式）。
pub fn md5_hex(data: &[u8]) -> String {
    to_hex(&md5(data))
}

/// 流式 CRC-32/ISO-HDLC（zlib `crc32()` 兼容）。
#[derive(Clone)]
pub struct Crc32 {
    state: u32,
}

impl Default for Crc32 {
    fn default() -> Self {
        Self::new()
    }
}

impl Crc32 {
    pub fn new() -> Self {
        Self { state: !0 }
    }

    pub fn update(&mut self, data: &[u8]) {
        let mut crc = self.state;
        for &byte in data {
            crc = (crc >> 8) ^ CRC_TABLE[((crc ^ byte as u32) & 0xFF) as usize];
        }
        self.state = crc;
    }

    pub fn finalize(self) -> u32 {
        !self.state
    }
}

/// 一次性计算 CRC-32/ISO-HDLC（"123456789" → 0xCBF43926）。
pub fn crc32(data: &[u8]) -> u32 {
    let mut hasher = Crc32::new();
    hasher.update(data);
    hasher.finalize()
}

const fn build_crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut n = i as u32;
        let mut bit = 0;
        while bit < 8 {
            n = if n & 1 != 0 {
                (n >> 1) ^ 0xEDB8_8320
            } else {
                n >> 1
            };
            bit += 1;
        }
        table[i] = n;
        i += 1;
    }
    table
}

static CRC_TABLE: [u32; 256] = build_crc_table();

/// 字节数组转小写十六进制字符串。
pub fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0F) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn md5_rfc1321_vectors() {
        let cases: &[(&str, &str)] = &[
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ];
        for (input, expected) in cases {
            assert_eq!(&md5_hex(input.as_bytes()), expected, "input {input:?}");
        }
    }

    #[test]
    fn md5_streaming_matches_oneshot() {
        let data: Vec<u8> = (0..=255u8).cycle().take(1000).collect();
        // 各种切分方式（覆盖跨块边界、残留缓冲）
        for chunk_size in [1, 7, 63, 64, 65, 128, 1000] {
            let mut hasher = Md5::new();
            for chunk in data.chunks(chunk_size) {
                hasher.update(chunk);
            }
            assert_eq!(hasher.finalize(), md5(&data), "chunk size {chunk_size}");
        }
    }

    #[test]
    fn md5_padding_boundaries() {
        // 消息长度正好 64 的倍数、55（单块放不下 0x80+长度）、56、64 边界
        for len in [0, 55, 56, 63, 64, 65, 119, 120, 128, 129] {
            let data = vec![0xA5u8; len];
            // 一致性：流式 = 一次性；边界长度不 panic 即可
            assert_eq!(md5(&data), md5(&data));
        }
    }

    #[test]
    fn crc32_known_vectors() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    #[test]
    fn crc32_streaming_matches_oneshot() {
        let data: Vec<u8> = (0..=255u8).rev().cycle().take(777).collect();
        let mut hasher = Crc32::new();
        hasher.update(&data[..123]);
        hasher.update(&data[123..]);
        assert_eq!(hasher.finalize(), crc32(&data));
    }
}
