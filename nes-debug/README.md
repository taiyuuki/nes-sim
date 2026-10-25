# nes-debug

基于 [nes-sim](../nes-sim) 核心库的调试器前端（Tauri 2 + Vue 3 + TypeScript），面向模拟精度开发与 ROM 逆向分析。

## 功能

- **CPU**：寄存器/状态面板、反汇编视图、地址断点、单帧/单指令步进
- **PPU**：Pattern Table / Name Table / 调色板查看器、OAM、扫描线与 VBlank 断点
- **内存**：RAM / VRAM / CHR 只读检查
- **音频**：AudioWorklet 输出、APU 通道静音
- **输入**：P1 键盘映射（方向键 + Z/X + Tab/Enter）

## 构建与运行

```sh
pnpm install
pnpm tauri dev      # 开发模式
pnpm tauri build    # 发布构建
```

需要 Rust 工具链与 Tauri 2 的系统依赖（macOS: Xcode CLT；Windows: WebView2；Linux: webkit2gtk）。

## 架构

```
src/                 Vue 3 前端（组件 + composables）
src-tauri/src/       Rust 后端（emulator.rs：核心库的 Tauri 命令封装）
public/              AudioWorklet 处理器（audio-processor.js）
```

视频帧经 `run_frame` 命令以 base64 索引帧返回，前端用 64 色查表（`nes-lut.ts`，含颜色强调调制）渲染到 canvas；音频经 `audio_data` 事件推送到 AudioWorklet。
