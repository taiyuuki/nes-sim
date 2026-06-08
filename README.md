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

## Building

```bash
# Core library (no external dependencies)
cargo build

# Run tests
cargo test
```

## Examples

| Example | Description |
|---|---|
| `desktop_frontend` | Full desktop GUI with audio sync, keyboard input, pause/step |
| `export_frame` | Export a single frame as PPM image |

```bash
cargo run --example export_frame -- "game.nes" "output.ppm" 180

cargo run --example desktop_frontend -- "game.nes""
```

## Supported Mappers

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
