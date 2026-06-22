# nes-sim

A NES (Famicom) emulator core library written in Rust. Headless design decouples the emulation engine from the frontend, making it easy to integrate into different UI frameworks or testing environments.

## Features

- **Complete CPU emulation** — 6502 processor with full addressing mode support and undocumented instructions
- **PPU rendering** — 256x240 resolution with sprite and background rendering
- **APU audio** — 5 standard channels (Pulse x2, Triangle, Noise, DMC)
- **Expansion audio** — VRC6, Namco 163, Sunsoft 5B, MMC5
- **Mappers** — Coverage of MMC1/MMC3/VRC series/Namco/Taito/Sunsoft and other common mappers
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

let mut nes = NES::new();
let rom = std::fs::read("game.nes")?;
nes.load_cartridge_ines(&rom)?;
nes.reset();

// Run one frame
nes.run_frame();

// Get indexed color pixel data (256x240)
let video = nes.video_frame();
let pixels = video.pixels; // &[u8], indexed 8-bit

// Get mono audio samples since last clear
let audio = nes.audio_batch();
let samples = audio.samples; // &[f32]
nes.clear_audio_samples();
```

## Project Structure

```
nes-sim/            Core library (zero dependencies)
nes-desktop/        Desktop frontend (minifb + cpal)
```

## Building

```bash
# Build core library
cargo build -p nes-sim

# Build desktop frontend
cargo build -p nes-desktop

# Run tests
cargo test -p nes-sim
```

## Examples

| Example | Description |
|---|---|
| `export_frame` | Export a single frame as PPM image |
| `analyze_audio` | Export audio as WAV |
| `analyze_state` | Analyze emulator state at a specific frame |
| `hash_frame` | Hash frame pixels for regression testing |

```bash
cargo run -p nes-sim --example export_frame -- "game.nes" "output.ppm" 180
```

## Desktop Frontend

```bash
cargo run --release -p nes-desktop -- "game.nes"
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
