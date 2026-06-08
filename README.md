# nes-sim

A NES (Famicom) emulator core library written in Rust. Headless design decouples the emulation engine from the frontend, making it easy to integrate into different UI frameworks or testing environments.

## Features

- **Complete CPU emulation** — 6502 processor with full addressing mode support and undocumented instructions
- **PPU rendering** — 256x240 resolution with sprite and background rendering
- **APU audio** — 5 standard channels (Pulse x2, Triangle, Noise, DMC)
- **Expansion audio** — VRC6, Namco 163, Sunsoft 5B, MMC5
- **45 Mappers** — Coverage of MMC1/MMC3/VRC series/Namco/Taito/Sunsoft and other common mappers
- **Multi-system support** — NTSC, PAL, DENDY
- **Save states** — Custom binary format with mapper validation
- **Debug support** — CPU/PPU state snapshots, per-channel mute
- **Zero-dependency core** — Core library has no external crate dependencies

## Quick Start

### Use as a library

```toml
[dependencies]
nes-sim = "0.1"
```

```rust
use nes_sim::NES;

let mut nes = NES::new("game.nes")?;
nes.reset();

// Run one frame (returns indexed color pixel data)
let frame = nes.clock();

// Output 44100Hz mono audio samples
let audio = nes.audio_samples();
```

### Desktop frontend

Enable the `desktop` feature (requires `minifb` + `cpal`):

```bash
cargo run --release --features desktop --example desktop_frontend -- "path/to/game.nes"
```

Or use the convenient script:

```bash
cargo run-desktop
```

## Building

```bash
# Core library (no external dependencies)
cargo build

# Desktop frontend
cargo build --release --features desktop --example desktop_frontend

# Run tests
cargo test
```

## Examples

| Example | Description |
|---|---|
| `desktop_frontend` | Full desktop GUI with audio sync, keyboard input, pause/step |
| `export_frame` | Export a single frame as PPM image |
| `hash_frame` | Compute FNV hash of rendered frame (for regression testing) |
| `analyze_state` | Analyze/save binary save state format |

```bash
# Export a frame
cargo run --example export_frame -- "game.nes" "output.ppm" 180

# Frame hash regression test
cargo run --example hash_frame -- "game.nes" 180 "current.ppm"
```

## Supported Mappers

0 (NROM), 1 (MMC1), 2 (UxROM), 3 (CNROM), 4 (MMC3), 5 (MMC5) 7 (AxROM), 11 (Color Dreams), 13 (CpROM), 19 (Namco 163), 21/23/25 (VRC4), 22 (VRC2), 24/26 (VRC6), 32 (Irem G-101), 33/48 (Taito TC0190), 34 (BNROM), 36, 46, 62, 65 (Irem H-3001), 66 (GxROM), 67 (Sunsoft 3), 69 (FME-7), 70, 71 (Camerica), 72, 76, 78, 79/113 (NINA-003), 80 (Taito X1-005), 82 (Taito X1-017), 86 (JF-13), 87, 88/154 (Namco 3433), 92 (JF-19), 94, 97 (Irem Tam S1), 115, 118 (TxSROM), 119 (TQROM), 152, 162

## Project Structure

```
src/
├── lib.rs          # Top-level NES struct and runtime loop
├── api.rs          # Public API types (commands/events/responses)
├── cpu.rs          # 6502 CPU
├── ppu.rs          # Picture Processing Unit
├── apu/            # Audio Processing Unit
│   ├── pulse.rs    #   Square wave channels x2
│   ├── triangle.rs #   Triangle wave channel
│   ├── noise.rs    #   Noise channel
│   └── dmc.rs      #   DPCM sample channel
├── bus.rs          # System bus
├── cartridge.rs    # ROM loading and mapper dispatch
├── mappers/        # 45 mapper implementations
├── expansion_audio/# Expansion audio chips
├── savestate.rs    # Save state serialization
├── video.rs        # Palette conversion
├── runtime.rs      # Frontend runtime abstraction
└── headless.rs     # Headless utilities (PPM export/FNV hash)
```

## Design Philosophy

- **Core-frontend separation** — `nes-sim` is a pure emulation library with no platform-specific code for rendering/audio/input
- **Command/event driven** — Controlled via `CoreCommand`/`CoreResponse` for easy embedding in different environments
- **Indexed color output** — Pixel format is 8-bit NES native palette index, frontend handles RGB conversion
- **Extreme release optimization** — Enabled `lto = "fat"` and `codegen-units = 1`