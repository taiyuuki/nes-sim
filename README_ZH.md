# nes-sim

用 Rust 编写的 NES（FC/红白机）模拟器核心库。采用 headless 设计，将模拟引擎与前端解耦，可方便地集成到不同的 UI 框架或测试环境中。

## 特性

- **完整的 CPU 模拟** — 6502 处理器，支持全部寻址模式及未授权指令
- **PPU 渲染** — 256x240 分辨率，精灵与背景渲染
- **APU 音频** — 5 个标准声道（Pulse x2、Triangle、Noise、DMC）
- **扩展音频** — VRC6、Namco 163、Sunsoft 5B、MMC5
- **45 种 Mapper** — 覆盖 MMC1/MMC3/VRC 系列/Namco/Taito/Sunsoft 等常见 Mapper
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

let mut nes = NES::new("game.nes")?;
nes.reset();

// 运行一帧（返回索引色像素数据）
let frame = nes.clock();

// 输出 44100Hz 单声道音频采样
let audio = nes.audio_samples();
```

## 构建

```bash
# 核心库（无外部依赖）
cargo build

# 运行测试
cargo test
```

## 示例

| 示例 | 说明 |
|---|---|
| `desktop_frontend` | 完整桌面 GUI，支持音频同步、键盘输入、暂停/单步 |
| `export_frame` | 导出单帧为 PPM 图片 |

```bash
cargo run --example export_frame -- "game.nes" "output.ppm" 180

cargo run --example desktop_frontend -- "game.nes""
```

## 支持的 Mapper

- 0 (NROM)
- 1 (MMC1)
- 2 (UxROM)
- 3 (CNROM)
- 4 (MMC3)
- 5 (MMC5)
- 7 (AxROM)
- 11 (Color Dreams)
- 13 (CpROM)
- 19 (Namco 163)
- 21/23/25 (VRC4)
- 22 (VRC2)
- 24/26 (VRC6)
- 32 (Irem G-101)
- 33/48 (Taito TC0190)
- 34 (BNROM)
- 36
- 46
- 62
- 65 (Irem H-3001)
- 66 (GxROM)
- 67 (Sunsoft 3)
- 69 (FME-7)
- 70
- 71 (Camerica)
- 72
- 76
- 78
- 79/113 (NINA-003)
- 80 (Taito X1-005)
- 82 (Taito X1-017)
- 86 (JF-13)
- 87
- 88/154 (Namco 3433)
- 92 (JF-19)
- 94
- 97 (Irem Tam S1)
- 115
- 118 (TxSROM)
- 119 (TQROM)
- 152
- 162

