# NES 模拟器调试接口文档

## 概述

核心库 `nes-sim` 提供了条件编译的调试功能，通过启用 `debug` feature 来访问。这些接口允许开发者查看和调试模拟器的内部状态。

## 启用调试功能

在 `Cargo.toml` 中启用 `debug` feature：

```toml
[dependencies]
nes-sim = { version = "0.1.2", features = ["debug"] }
```

或者在命令行中启用：

```bash
cargo build --features debug
cargo test --features debug
```

## 调试 API

### 内存快照

`MemorySnapshot` 结构提供对所有主要内存区域的只读访问：

```rust
pub struct MemorySnapshot<'a> {
    pub ram: &'a [u8; 0x800],      // CPU RAM (2KB, 镜像到 $0000-$1FFF)
    pub vram: &'a [u8; 0x1000],    // PPU VRAM/命名表 (4KB)
    pub chr: &'a [u8; 0x2000],     // CHR RAM/ROM (8KB, 图案表)
    pub palette: &'a [u8; 0x20],   // 调色板 RAM (32 字节)
    pub oam: &'a [u8; 256],        // OAM (256 字节, 64 个精灵)
}
```

**使用示例：**

```rust
let nes = NES::new();
let memory = nes.debug_memory_snapshot();

// 读取 CPU RAM
let stack_top = memory.ram[0x01FF];

// 读取 PPU 调色板
let background_color = memory.palette[0x00];

// 读取 OAM 精灵数据
let sprite_0_y = memory.oam[0];
let sprite_0_tile = memory.oam[1];
let sprite_0_attr = memory.oam[2];
let sprite_0_x = memory.oam[3];
```

### PPU 调试快照

`PpuDebugSnapshot` 提供了详细的 PPU 状态信息。当启用 `debug` feature 时，包含额外字段：

```rust
pub struct PpuDebugSnapshot {
    // 基础字段 (始终可用)
    pub frame: u64,           // 帧计数
    pub scanline: i16,        // 当前扫描线
    pub in_vblank: bool,      // 是否在垂直消隐期
    pub nmi_line: bool,       // NMI 线状态
    pub oam_addr: u8,         // OAM 地址寄存器

    // 仅在 debug feature 启用时可用
    pub cycles: u16,          // PPU 周期 (0-340)
    pub ctrl: u8,             // PPUCTRL ($2000)
    pub mask: u8,             // PPUMASK ($2001)
    pub status: u8,           // PPUSTATUS ($2002)
    pub fine_x: u8,           // 细粒度 X 滚动 (0-7)
    pub vram_addr: u16,       // 当前 VRAM 地址 (Loopy V)
    pub temp_vram_addr: u16,  // 临时 VRAM 地址 (Loopy T)
    pub write_latch: bool,    // 写入锁存器状态
    pub bg_on: bool,          // 背景渲染启用
    pub sprites_on: bool,     // 精灵渲染启用
    pub rendering_on: bool,   // 渲染启用
    pub odd_frame: bool,      // 奇数帧标志
}
```

**使用示例：**

```rust
let snapshot = nes.debug_snapshot();

let ppu = snapshot.ppu;

// 检查渲染状态
if ppu.rendering_on {
    println!("背景: {}, 精灵: {}", ppu.bg_on, ppu.sprites_on);
}

// 读取 PPUCTRL 配置
#[cfg(feature = "debug")]
{
    let sprite_table = if (ppu.ctrl & 0x08) != 0 { 0x1000 } else { 0x0000 };
    let bg_table = if (ppu.ctrl & 0x10) != 0 { 0x1000 } else { 0x0000 };
    println!("精灵表: ${:04X}, 背景表: ${:04X}", sprite_table, bg_table);
}
```

### PPU 寄存器位解析

**PPUCTRL ($2000):**
- Bit 0-1: 名称表地址
- Bit 2: VRAM 地址增量 (0=+1, 1=+32)
- Bit 3: 精灵图案表地址 (0=$0000, 1=$1000)
- Bit 4: 背景图案表地址 (0=$0000, 1=$1000)
- Bit 5: 精灵大小 (0=8×8, 1=8×16)
- Bit 6: PPU 主/从模式
- Bit 7: 生成 NMI

**PPUMASK ($2001):**
- Bit 0: 灰度模式
- Bit 1: 显示背景左列
- Bit 2: 显示精灵左列
- Bit 3: 显示背景
- Bit 4: 显示精灵
- Bit 5: 强调红色
- Bit 6: 强调绿色
- Bit 7: 强调蓝色

**PPUSTATUS ($2002):**
- Bit 5: 精灵溢出
- Bit 6: 精灵 0 命中
- Bit 7: VBlank 开始

### 断点系统

`Breakpoint` 枚举定义了五种断点类型：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breakpoint {
    Address(u16),           // PC 断点：CPU 执行到指定地址时暂停
    MemoryRead(u16),        // 内存读取断点：CPU 读取指定地址时暂停
    MemoryWrite(u16),       // 内存写入断点：CPU 写入指定地址时暂停
    PpuScanline(i16),       // PPU 扫描线断点：PPU 到达指定扫描线时暂停
    Vblank,                 // VBlank 断点：进入 VBlank 时暂停
}
```

#### 断点管理 API

```rust
impl NES {
    // 添加断点（重复添加会被忽略）
    pub fn add_breakpoint(&mut self, bp: Breakpoint);

    // 移除断点
    pub fn remove_breakpoint(&mut self, bp: &Breakpoint);

    // 清除所有断点
    pub fn clear_breakpoints(&mut self);

    // 获取当前断点列表
    pub fn breakpoints(&self) -> &[Breakpoint];

    // 手动暂停/恢复
    pub fn set_paused(&mut self, paused: bool);

    // 是否处于暂停状态（手动暂停或断点命中）
    pub fn paused(&self) -> bool;

    // 获取命中的断点（如果有）
    pub fn breakpoint_hit(&self) -> Option<Breakpoint>;
}
```

#### 断点工作原理

当 `NES::clock()` 被调用时：

1. **暂停检查** — 如果 `paused()` 返回 `true`（手动暂停或之前有断点命中），`clock()` 立即返回，不推进任何状态
2. **PPU 断点** — 每个 PPU tick 后检查扫描线和 VBlank 断点
3. **内存断点** — 在 `NESBus::cpu_read_internal` / `cpu_write_internal` 中实时检查，命中时记录到内部标志
4. **PC 断点** — 在 CPU 指令执行完成后检查当前 PC 值
5. **命中处理** — 断点命中后设置 `breakpoint_hit`，后续 `clock()` 调用不再推进

#### 典型调试流程

```rust
// 1. 设置断点
nes.add_breakpoint(Breakpoint::Address(0xC000));
nes.add_breakpoint(Breakpoint::MemoryWrite(0x0200));

// 2. 运行直到命中
while !nes.paused() {
    nes.clock();
}

// 3. 检查命中了什么
if let Some(bp) = nes.breakpoint_hit() {
    match bp {
        Breakpoint::Address(addr) => println!("PC 断点命中: ${:04X}", addr),
        Breakpoint::MemoryRead(addr) => println!("内存读取断点命中: ${:04X}", addr),
        Breakpoint::MemoryWrite(addr) => println!("内存写入断点命中: ${:04X}", addr),
        Breakpoint::PpuScanline(sl) => println!("扫描线断点命中: {}", sl),
        Breakpoint::Vblank => println!("VBlank 断点命中"),
    }

    // 查看当前状态
    let snapshot = nes.debug_snapshot();
    println!("CPU: A={:02X} X={:02X} Y={:02X} SP={:02X} PC={:04X}",
        snapshot.cpu.a, snapshot.cpu.x, snapshot.cpu.y,
        snapshot.cpu.sp, snapshot.cpu.pc);

    let mem = nes.debug_memory_snapshot();
    println!("RAM[$0200] = {:02X}", mem.ram[0x200]);
}

// 4. 单步执行一条 CPU 指令
nes.step_cpu_instruction();

// 5. 清除断点并恢复运行
nes.set_paused(false);
nes.clear_breakpoints();
```

#### 通过 CoreCommand 使用

断点操作也集成到了 `CoreCommand` 枚举中，可以通过 `execute()` 调用：

```rust
nes.execute(CoreCommand::AddBreakpoint(Breakpoint::Address(0xC000)));
nes.execute(CoreCommand::SetPaused(false));

// 适合与 Runtime 配合使用的场景
```

## 内存布局参考

### CPU 地址映射

| 地址范围 | 大小 | 描述 |
|---------|------|------|
| $0000-$07FF | 2KB | CPU RAM (镜像到 $1FFF) |
| $2000-$2007 | 8B | PPU 寄存器 (镜像到 $3FFF) |
| $4000-$4017 | 24B | APU 和 I/O 寄存器 |
| $4018-$401F | 8B | APU 和 I/O 测试模式 |
| $4020-$FFFF | - | 卡带空间 |

### PPU 地址映射

| 地址范围 | 大小 | 描述 |
|---------|------|------|
| $0000-$0FFF | 4KB | 图案表 0 (或 1K CHR banks) |
| $1000-$1FFF | 4KB | 图案表 1 (或 1K CHR banks) |
| $2000-$23FF | 1KB | 名称表 0 |
| $2400-$27FF | 1KB | 名称表 1 |
| $2800-$2BFF | 1KB | 名称表 2 |
| $2C00-$2FFF | 1KB | 名称表 3 |
| $3000-$3EFF | - | 名称表镜像 |
| $3F00-$3F0F | 16B | 背景调色板 |
| $3F10-$3F1F | 16B | 精灵调色板 |

### OAM 布局

每个精灵占用 4 字节：

| 偏移 | 字段 | 描述 |
|------|------|------|
| 0 | Y | 精灵 Y 坐标 |
| 1 | Tile | 图块编号 |
| 2 | Attr | 属性字节 (位 0-1: 调色板, 位 5: 优先级, 位 6: 水平翻转, 位 7: 垂直翻转) |
| 3 | X | 精灵 X 坐标 |

## 性能注意事项

1. **零成本抽象** — 不启用 `debug` feature 时，调试代码不会编译到最终二进制中，`clock()` 无额外开销
2. **引用语义** — `MemorySnapshot` 使用引用而非复制，避免大型数组的内存复制
3. **条件编译** — PPU 扩展字段和断点逻辑仅在 `debug` feature 启用时存在
4. **内存断点** — 每个 CPU 读/写操作会遍历内存断点列表，断点较多时有轻微性能影响