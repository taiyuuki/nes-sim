// nes-wasm Node 冒烟测试:加载 ROM 跑几帧,检查帧缓冲、音频、存档。
// 默认尝试本地 fixture ROM；不存在时合成一个显示全屏红的最小 NROM（CI 用）。
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { default: init, NesWasm } = require("./pkg/nes_wasm.js");

const root = path.dirname(fileURLToPath(import.meta.url));
const defaultRom = path.join(root, "../nes-sim/roms/mapper_065/Kaiketsu Yanchamaru 3 (J) [!].nes");

// 合成 NROM: 写调色板背景色 $16(红) 并开启渲染，CHR 全零 → 全屏红。
function synthNrom() {
  const code = [
    0xA9, 0x3F, 0x8D, 0x06, 0x20, // LDA #$3F; STA $2006
    0xA9, 0x00, 0x8D, 0x06, 0x20, // LDA #$00; STA $2006  (VRAM=$3F00)
    0xA9, 0x16, 0x8D, 0x07, 0x20, // LDA #$16; STA $2007  (palette[0]=红)
    0xA9, 0x00, 0x8D, 0x05, 0x20, // LDA #$00; STA $2005
    0x8D, 0x05, 0x20,             // STA $2005
    0xA9, 0x1E, 0x8D, 0x01, 0x20, // LDA #$1E; STA $2001  (开渲染)
    0x4C, 0x1B, 0x80,             // JMP $801B
  ];
  const prg = new Uint8Array(16384);
  prg.set(code, 0);
  prg[0x3FFC] = 0x00; // reset vector = $8000
  prg[0x3FFD] = 0x80;
  const header = new Uint8Array([0x4E, 0x45, 0x53, 0x1A, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
  const rom = new Uint8Array(16 + 16384 + 8192);
  rom.set(header, 0);
  rom.set(prg, 16);
  return rom;
}

const romArg = process.argv[2];
let rom;
let romName;
if (romArg) {
  rom = new Uint8Array(readFileSync(romArg));
  romName = path.basename(romArg);
} else if (existsSync(defaultRom)) {
  rom = new Uint8Array(readFileSync(defaultRom));
  romName = path.basename(defaultRom);
} else {
  rom = synthNrom();
  romName = "synth-nrom.nes";
  console.log("fixture ROM not found, using synthesized NROM");
}

await init({ module_or_path: readFileSync(path.join(root, "pkg/nes_wasm_bg.wasm")) });

const nes = new NesWasm();
nes.load_rom(rom);
console.log("loaded:", romName);

let failures = 0;
function check(ok, label) {
  console.log((ok ? "OK  " : "FAIL") + " " + label);
  if (!ok) failures++;
}

// 跑 60 帧
let t0 = performance.now();
let lastFrame = 0;
for (let i = 0; i < 60; i++) lastFrame = nes.run_frame();
const elapsed = performance.now() - t0;
console.log(`60 frames in ${elapsed.toFixed(1)}ms (${(60000 / elapsed).toFixed(0)} fps emulated)`);
check(lastFrame >= 60, "frame counter advanced");

// 帧缓冲
const rgba = nes.frame_rgba();
check(rgba.length === 256 * 240 * 4, `rgba length ${rgba.length}`);
const nonBlack = Array.from(rgba).filter((v, i) => i % 4 !== 3 && v !== 0).length;
check(nonBlack > 0, `frame is not blank (${nonBlack} non-black color bytes)`);

const indexed = nes.frame_pixels();
check(indexed.length === 256 * 240, `indexed length ${indexed.length}`);

// 调色板
const pal = nes.palette_rgb();
check(pal.length === 192 && pal[0] === 84 && pal[1] === 84 && pal[2] === 84, "palette rgb");

// 音频
const audio = nes.audio_samples();
check(nes.audio_sample_rate() > 0 && audio.length > 0, `audio samples ${audio.length} @ ${nes.audio_sample_rate()}Hz`);
nes.clear_audio_samples();

// 手柄
nes.set_button(0, "Start", true);
check(nes.controller_bits(0) === 8, "controller Start bit");
nes.set_button(0, "A", true);
check(nes.controller_bits(0) === 9, "controller Start+A bits");
nes.set_controller_bits(0, 0);
check(nes.controller_bits(0) === 0, "controller cleared");

// 存档/读档
const state = nes.save_state();
check(state.length > 0, `save state ${state.length} bytes`);
nes.run_frame();
nes.load_state(state);
check(nes.frame_number() > 0, "load state");

// MD5
check(nes.rom_md5_hex()?.length === 32, "rom md5");
check(nes.master_clock() > 0, "master clock");

// 自定义调色板: 64 色全部填纯蓝,加载后帧应变成蓝色,clear 后恢复
const bluePal = new Uint8Array(192);
for (let i = 0; i < 64; i++) {
  bluePal[i * 3] = 0;
  bluePal[i * 3 + 1] = 0;
  bluePal[i * 3 + 2] = 255;
}
nes.set_palette(bluePal);
check(nes.palette_rgb()[2] === 255 && nes.palette_rgb()[0] === 0, "set_palette 64-color base");
check(nes.palette_rgb_full().length === 1536, "palette_rgb_full 512 entries");
const blueFrame = nes.frame_rgba();
check(blueFrame[0] === 0 && blueFrame[1] === 0 && blueFrame[2] === 255, "frame_rgba uses custom palette");
nes.clear_palette();
const defaultFrame = nes.frame_rgba();
check(defaultFrame[0] === 84 && defaultFrame[1] === 84 && defaultFrame[2] === 84, "clear_palette restores default");

console.log(failures === 0 ? "\nALL CHECKS PASSED" : `\n${failures} CHECK(S) FAILED`);
process.exit(failures === 0 ? 0 : 1);
