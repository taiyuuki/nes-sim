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
nes-sim = { path = "../nes_rs" }
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

### 桌面前端

需要启用 `desktop` feature（依赖 `minifb` + `cpal`）：

```bash
cargo run --release --features desktop --example desktop_frontend -- "path/to/game.nes"
```

或使用便捷脚本：

```bash
cargo run-desktop
```

## 构建

```bash
# 核心库（无外部依赖）
cargo build

# 桌面前端
cargo build --release --features desktop --example desktop_frontend

# 运行测试
cargo test
```

## 示例

| 示例 | 说明 |
|---|---|
| `desktop_frontend` | 完整桌面 GUI，支持音频同步、键盘输入、暂停/单步 |
| `export_frame` | 导出单帧为 PPM 图片 |
| `hash_frame` | 计算渲染帧的 FNV 哈希（用于回归测试） |
| `analyze_state` | 分析/保存二进制存档格式 |

```bash
# 导出一帧画面
cargo run --example export_frame -- "game.nes" "output.ppm" 180

# 帧哈希回归测试
cargo run --example hash_frame -- "game.nes" 180 "current.ppm"
```

## 支持的 Mapper

0 (NROM), 1 (MMC1), 2 (UxROM), 3 (CNROM), 4 (MMC3), 5 (MMC5) 7 (AxROM), 11 (Color Dreams), 13 (CpROM), 19 (Namco 163), 21/23/25 (VRC4), 22 (VRC2), 24/26 (VRC6), 32 (Irem G-101), 33/48 (Taito TC0190), 34 (BNROM), 36, 46, 62, 65 (Irem H-3001), 66 (GxROM), 67 (Sunsoft 3), 69 (FME-7), 70, 71 (Camerica), 72, 76, 78, 79/113 (NINA-003), 80 (Taito X1-005), 82 (Taito X1-017), 86 (JF-13), 87, 88/154 (Namco 3433), 92 (JF-19), 94, 97 (Irem Tam S1), 115, 118 (TxSROM), 119 (TQROM), 152, 162

## 项目结构

```
src/
├── lib.rs          # 顶层 NES 结构体与运行循环
├── api.rs          # 公共 API 类型（命令/事件/响应）
├── cpu.rs          # 6502 CPU
├── ppu.rs          # 图形处理单元
├── apu/            # 音频处理单元
│   ├── pulse.rs    #   方波通道 x2
│   ├── triangle.rs #   三角波通道
│   ├── noise.rs    #   噪声通道
│   └── dmc.rs      #   DPCM 采样通道
├── bus.rs          # 系统总线
├── cartridge.rs    # ROM 加载与 Mapper 调度
├── mappers/        # 45 种 Mapper 实现
├── expansion_audio/# 扩展音频芯片
├── savestate.rs    # 存档序列化
├── video.rs        # 调色板转换
├── runtime.rs      # 前端运行时抽象
└── headless.rs     # 无头模式工具（PPM 导出/FNV 哈希）
```

## 设计理念

- **核心与前端分离** — `nes-sim` 是纯模拟库，不包含任何渲染/音频/输入的平台相关代码
- **命令/事件驱动** — 通过 `CoreCommand` / `CoreResponse` 进行控制，方便嵌入不同环境
- **索引色输出** — 像素格式为 8 位 NES 原生调色板索引，由前端负责转 RGB
- **Release 极致优化** — 启用 `lto = "fat"` 和 `codegen-units = 1`
