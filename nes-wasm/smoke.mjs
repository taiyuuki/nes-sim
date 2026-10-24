// nes-wasm Node 冒烟测试:加载 ROM 跑几帧,检查帧缓冲、音频、存档。
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const { default: init, NesWasm } = require("./pkg/nes_wasm.js");

const root = path.dirname(fileURLToPath(import.meta.url));
const romPath = process.argv[2]
  ?? path.join(root, "../nes-sim/roms/mapper_065/Kaiketsu Yanchamaru 3 (J) [!].nes");

await init({ module_or_path: readFileSync(path.join(root, "pkg/nes_wasm_bg.wasm")) });

const nes = new NesWasm();
const rom = readFileSync(romPath);
nes.load_rom(new Uint8Array(rom));
console.log("loaded:", path.basename(romPath));

// 跑 60 帧
let t0 = performance.now();
let lastFrame = 0;
for (let i = 0; i < 60; i++) lastFrame = nes.run_frame();
const elapsed = performance.now() - t0;
console.log(`60 frames in ${elapsed.toFixed(1)}ms (${(60000 / elapsed).toFixed(0)} fps emulated)`);

// 帧缓冲
const rgba = nes.frame_rgba();
console.log("rgba len:", rgba.length, "(expect 256*240*4 =", 256 * 240 * 4, ")");
const nonBlack = Array.from(rgba).filter((v, i) => i % 4 !== 3 && v !== 0).length;
console.log("non-black color bytes:", nonBlack, nonBlack > 0 ? "OK" : "FAIL(blank frame)");

const indexed = nes.frame_pixels();
console.log("indexed len:", indexed.length, "(expect", 256 * 240, ")");

// 调色板
const pal = NesWasm.palette_rgb();
console.log("palette:", pal.length, "bytes, first color:", pal[0], pal[1], pal[2], "(expect 84 84 84)");

// 音频
console.log("sample rate:", nes.audio_sample_rate());
const audio = nes.audio_samples();
console.log("audio samples:", audio.length, "first:", audio[0]?.toFixed(4));
nes.clear_audio_samples();

// 手柄
nes.set_button(0, "Start", true);
console.log("controller bits after Start:", nes.controller_bits(0), "(expect 8)");
nes.set_button(0, "A", true);
console.log("controller bits after +A:", nes.controller_bits(0), "(expect 9)");
nes.set_controller_bits(0, 0);
console.log("cleared:", nes.controller_bits(0), "(expect 0)");

// 存档/读档
const state = nes.save_state();
console.log("save state:", state.length, "bytes");
nes.run_frame();
nes.load_state(state);
console.log("load state OK, frame:", nes.frame_number());

// MD5
console.log("rom md5:", nes.rom_md5_hex());
console.log("master clock:", nes.master_clock());

// 电池 RAM(该卡带可能无)
console.log("battery ram:", nes.battery_ram());

console.log("\nALL CHECKS DONE");
