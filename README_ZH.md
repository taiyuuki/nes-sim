# nes-sim

用 Rust 编写的 NES（FC/红白机）模拟器核心库。采用 headless 设计，将模拟引擎与前端解耦，可方便地集成到不同的 UI 框架或测试环境中。

## 特性

- **完整的 CPU 模拟** — 6502 处理器，支持全部寻址模式及未授权指令
- **PPU 渲染** — 256x240 分辨率，精灵与背景渲染
- **APU 音频** — 5 个标准声道（Pulse x2、Triangle、Noise、DMC）
- **扩展音频** — VRC6、Namco 163、Sunsoft 5B、MMC5
- **Mapper** — 覆盖 MMC1/MMC3/VRC 系列/Namco/Taito/Sunsoft 等常见 Mapper
- **多制式支持** — NTSC、PAL、DENDY
- **存档功能** — 自定义二进制格式，支持 Mapper 校验
- **调试支持** — CPU/PPU 状态快照、声道独立静音
- **零依赖核心** — 核心库不依赖任何外部 crate

## 快速开始

### 作为库使用

```toml
[dependencies]
nes-sim = "0.1"
```

```rust
use nes_sim::NES;

let mut nes = NES::new();
let rom = std::fs::read("game.nes")?;
nes.load_cartridge_ines(&rom)?;
nes.reset();

// 运行一帧
nes.run_frame();

// 获取索引色像素数据（256x240）
let video = nes.video_frame();
let pixels = video.pixels; // &[u8]，8位索引色

// 获取上次清空后的单声道音频采样
let audio = nes.audio_batch();
let samples = audio.samples; // &[f32]
nes.clear_audio_samples();
```

## 项目结构

```
nes-sim/            核心库（零依赖）
nes-desktop/        桌面前端（minifb + cpal）
```

## 构建

```bash
# 构建核心库
cargo build -p nes-sim

# 构建桌面前端
cargo build -p nes-desktop

# 运行测试
cargo test -p nes-sim
```

## 示例

| 示例 | 说明 |
|---|---|
| `export_frame` | 导出单帧为 PPM 图片 |
| `analyze_audio` | 导出音频为 WAV |
| `analyze_state` | 分析指定帧的模拟器状态 |
| `hash_frame` | 哈希帧像素用于回归测试 |

```bash
cargo run -p nes-sim --example export_frame -- "game.nes" "output.ppm" 180
```

## 桌面前端

```bash
cargo run --release -p nes-desktop -- "game.nes"
```

## 支持的 Mapper

- 0 (NROM)
- 1 (MMC1)
- 2 (UxROM)
- 3 (CNROM)
- 4 (MMC3)
- 5 (MMC5)
- 7 (AxROM)
- 9 (MMC2)
- 10 (MMC4)
- 11 (Color Dreams)
- 13 (CpROM)
- 15
- 16 (Bandai)
- 18
- 19 (Namco 163)
- 21/23/25 (VRC4)
- 22 (VRC2)
- 24/26 (VRC6)
- 32 (Irem G-101)
- 33/48 (Taito TC0190)
- 34 (BNROM)
- 36
- 37 (ZZ multicart)
- 38
- 46
- 47 (QJ multicart)
- 48
- 49 (SuperHiK 4-in-1)
- 62
- 64
- 65 (Irem H-3001)
- 66 (GxROM)
- 67 (Sunsoft 3)
- 68
- 69 (FME-7)
- 70
- 71 (Camerica)
- 72
- 73
- 74
- 75
- 76
- 78
- 79/113 (NINA-003)
- 80 (Taito X1-005)
- 82 (Taito X1-017)
- 85
- 86 (JF-13)
- 87
- 88/154 (Namco 3433)
- 89
- 92 (JF-19)
- 93
- 94
- 97 (Irem Tam S1)
- 105 (Nintendo World Championship)
- 107
- 112
- 113
- 115
- 118 (TxSROM)
- 119 (TQROM)
- 140
- 152
- 154
- 162
- 180
- 182
- 184
- 185
- 200 (BMC 36-in-1)
- 201 (BMC 21-in-1)
- 206 (DE1ROM)
- 225 (BMC 72-in-1)
- 226 (BMC 76-in-1)
- 229 (BMC 31-in-1)
- 240 (CNE Shui Hu Zhuan)
- 241 (TXC MXMDHTWO)
- 242 (Waixing DQVII)
- 244 (CNE Decathlon)
