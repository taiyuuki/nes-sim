# nes-wasm

[nes-sim](https://crates.io/crates/nes-sim) 核心库的 WebAssembly 绑定(wasm-bindgen)。纯 Rust 实现的 NES/FC 模拟器核心,无依赖,可用于浏览器、Node 及各类 JS 运行时。

## 安装

```sh
npm install nes-wasm
```

```js
import init, { NesWasm } from "nes-wasm";
await init();
```

## 本地构建

```sh
wasm-pack build nes-wasm --target web --release
# 产物输出到 nes-wasm/pkg/
```

其他 target:`--target bundler`(webpack/vite)、`--target web`(原生 ESM)、`--target nodejs`。

## 使用示例

```js
import init, { NesWasm } from "./pkg/nes_wasm.js"; // npm 安装时: from "nes-wasm"

await init(); // 或传 URL / 字节: init({ module_or_path: bytes })

const nes = new NesWasm();
nes.load_rom(new Uint8Array(romBytes));   // iNES;FDS 用 load_fds(image, bios)

const canvas = document.querySelector("canvas");
canvas.width = 256;
canvas.height = 240;
const ctx = canvas.getContext("2d");
const imageData = ctx.createImageData(256, 240);

// 手柄位掩码:A=1 B=2 Select=4 Start=8 Up=16 Down=32 Left=64 Right=128
nes.set_controller_bits(0, 0x08 | 0x10); // Start + Up
// 或按名称:nes.set_button(0, "Start", true)

function loop() {
  nes.run_frame();                        // 模拟一帧
  imageData.data.set(nes.frame_rgba());   // RGBA 帧缓冲
  ctx.putImageData(imageData, 0, 0);

  // 音频(单声道 f32,采样率见 nes.audio_sample_rate())
  const samples = nes.audio_samples();
  nes.clear_audio_samples();

  requestAnimationFrame(loop);
}
loop();
```

## API 摘要

| 方法 | 说明 |
| --- | --- |
| `load_rom(rom)` / `load_fds(image, bios)` | 加载 ROM |
| `reset()` / `power_cycle()` | 复位 / 断电重启 |
| `run_frame()` | 运行一帧,返回帧号 |
| `frame_rgba()` | RGBA 帧缓冲(256×240×4),可直接写 `ImageData` |
| `frame_pixels()` | 8 位调色板索引帧;`palette_rgb()` 为 64 色系统调色板 |
| `set_palette(bytes)` / `clear_palette()` | 加载/重置 `.pal` 调色板(FCEUX/Nestopia 格式,192=64 色或 1536=512 色);`palette_rgb_full()` 返回当前 512×3 字节 |
| `audio_samples()` / `clear_audio_samples()` / `audio_sample_rate()` / `set_audio_sample_rate(rate)` | 音频 |
| `set_controller_bits(port, bits)` / `set_button(port, name, pressed)` | 手柄输入 |
| `save_state()` / `load_state(bytes)` | 快照存读档 |
| `battery_ram()` / `load_battery_ram(data)` | 电池 RAM(.sav) |
| `fds_dirty()` / `fds_sides_flattened()` / `fds_side_lengths()` | FDS 磁盘写回 |
| `rom_md5_hex()` / `frame_number()` / `master_clock()` | 元信息 |

冒烟测试:`node nes-wasm/smoke.mjs [rom路径]`(需先构建 pkg)。
