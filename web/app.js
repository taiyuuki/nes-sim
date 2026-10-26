// NES·SIM 网页演示 — 音频时钟驱动逐帧模拟
//
// 时序策略:模拟进度由音频队列水位驱动(rAF 中补帧),
// 对 60/120/144Hz 显示器都能以真实速度运行;
// rAF 同时负责把最新帧贴到 canvas。

import init, { NesWasm } from "./pkg/nes_wasm.js";

const $ = (sel) => document.querySelector(sel);

// ── 状态 ────────────────────────────────────────────────

const canvas = $("#screen");
const ctx2d = canvas.getContext("2d");
ctx2d.imageSmoothingEnabled = false;

// 离屏原生分辨率画布(256×240),再整数放大到显示画布
const off = document.createElement("canvas");
off.width = 256;
off.height = 240;
const offCtx = off.getContext("2d");
const imageData = offCtx.createImageData(256, 240);

let nes = null;
let romLoaded = false;
let paused = false;
let romKey = null; // md5 hex,存档隔离用

// 音频
let audioCtx = null;
let gainNode = null;
let muted = false;
let samplesPerFrame = 735; // 44100/60
let audioQueue = [];       // Float32Array 队列
let queueHead = null;
let headPos = 0;
let queued = 0;
const QUEUE_TARGET = 4;    // 队列水位:保持约 4 帧采样

// 输入
const KEY_MAP = {
  ArrowUp: "Up", KeyW: "Up",
  ArrowDown: "Down", KeyS: "Down",
  ArrowLeft: "Left", KeyA: "Left",
  ArrowRight: "Right", KeyD: "Right",
  KeyZ: "B", KeyJ: "B",
  KeyX: "A", KeyK: "A",
  Enter: "Start",
  ShiftLeft: "Select", ShiftRight: "Select",
};
const BTN_BITS = { A: 1, B: 2, Select: 4, Start: 8, Up: 16, Down: 32, Left: 64, Right: 128 };
const pressed = new Set();
let lastBits = 0;

// 统计
let fpsFrames = 0;
let fpsTime = 0;

// 倒带:每模拟帧存一份快照,按住时回退
const REWIND_MAX_FRAMES = 360; // ~6 秒 @60fps
const REWIND_SPEED = 3;        // 每渲染帧回退的模拟帧数(~3×速)
const rewindBuf = [];          // Uint8Array 环形缓冲
let rewinding = false;

// 自制 ROM 标题美化(文件名 → 展示名)
const TITLES = {
  "altercation-0.1.nes": "Altercation",
  "arkade-rush.nes": "Arkade Rush",
  "Atmo Sphere.nes": "Atmo Sphere",
  "bare-metal-6b2e249b.nes": "Bare Metal",
  "blobert.nes": "Blobert",
  "BlobQuest.nes": "Blob Quest",
  "Chocoblip_Alpha_0.1.nes": "Chocoblip",
  "FINes.nes": "FINes",
  "guntner(29).nes": "Guntner",
  "Immunatio (nesdemia).nes": "Immunatio",
  "nogoodchoice.nes": "No Good Choice",
  "Spacegulls-1.1.nes": "Spacegulls",
  "trouble_at_2a03-nesdev-submission-build.nes": "Trouble at 2A03",
  "Upsad_down_v0_6.nes": "Upsad Down",
  "witchnwiz_2021_02_22_nes_dev_v_1_0_0.nes": "Witch n' Wiz",
  "WolfSpirit.nes": "Wolf Spirit",
};

// ── 工具 ────────────────────────────────────────────────

let toastTimer = 0;
function toast(msg, isError = false) {
  const el = $("#toast");
  el.textContent = msg;
  el.classList.toggle("error", isError);
  el.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.remove("show"), 2200);
}

function prettify(filename) {
  if (TITLES[filename]) return TITLES[filename];
  return filename.replace(/\.nes$/i, "").replace(/[_-]+/g, " ").trim();
}

// ── 音频 ────────────────────────────────────────────────

function ensureAudio() {
  if (audioCtx) {
    if (audioCtx.state === "suspended" && !paused) audioCtx.resume();
    return;
  }
  try {
    audioCtx = new (window.AudioContext || window.webkitAudioContext)();
  } catch {
    return; // 无音频环境也能玩
  }
  nes.set_audio_sample_rate(audioCtx.sampleRate);
  samplesPerFrame = Math.ceil(audioCtx.sampleRate / 60);
  gainNode = audioCtx.createGain();
  gainNode.gain.value = muted ? 0 : 1;
  const proc = audioCtx.createScriptProcessor(1024, 0, 1);
  proc.onaudioprocess = (e) => pullSamples(e.outputBuffer.getChannelData(0));
  gainNode.connect(audioCtx.destination);
  proc.connect(gainNode);
}

function pushSamples(chunk) {
  if (chunk.length === 0) return;
  audioQueue.push(chunk);
  queued += chunk.length;
}

function pullSamples(out) {
  let pos = 0;
  while (pos < out.length) {
    if (!queueHead) {
      if (audioQueue.length === 0) break;
      queueHead = audioQueue.shift();
      headPos = 0;
    }
    const take = Math.min(out.length - pos, queueHead.length - headPos);
    out.set(queueHead.subarray(headPos, headPos + take), pos);
    headPos += take;
    pos += take;
    queued -= take;
    if (headPos >= queueHead.length) { queueHead = null; headPos = 0; }
  }
  if (pos < out.length) out.fill(0, pos); // 欠载:输出静音
}

function clearAudioQueue() {
  audioQueue.length = 0;
  queueHead = null;
  headPos = 0;
  queued = 0;
}

// ── 输入 ────────────────────────────────────────────────

function currentBits() {
  let bits = 0;
  for (const name of pressed) bits |= BTN_BITS[name] || 0;

  // Gamepad API(标准映射)
  for (const gp of navigator.getGamepads?.() || []) {
    if (!gp) continue;
    const b = gp.buttons;
    if (b[0]?.pressed) bits |= BTN_BITS.A;       // 底键 → A
    if (b[1]?.pressed || b[2]?.pressed) bits |= BTN_BITS.B;
    if (b[8]?.pressed) bits |= BTN_BITS.Select;
    if (b[9]?.pressed) bits |= BTN_BITS.Start;
    if (b[12]?.pressed) bits |= BTN_BITS.Up;
    if (b[13]?.pressed) bits |= BTN_BITS.Down;
    if (b[14]?.pressed) bits |= BTN_BITS.Left;
    if (b[15]?.pressed) bits |= BTN_BITS.Right;
    break; // 只认第一个手柄(1P)
  }
  return bits;
}

window.addEventListener("keydown", (e) => {
  if (e.key === "p" || e.key === "P") { togglePause(); return; }
  if (e.code === "Backspace") { e.preventDefault(); startRewind(); return; }
  const name = KEY_MAP[e.code];
  if (!name) return;
  e.preventDefault();
  pressed.add(name);
});

window.addEventListener("keyup", (e) => {
  if (e.code === "Backspace") { stopRewind(); return; }
  const name = KEY_MAP[e.code];
  if (name) pressed.delete(name);
});

// 页面失焦时清空按键,避免“卡键”
window.addEventListener("blur", () => { pressed.clear(); stopRewind(); });

// ── 触屏虚拟手柄 ──────────────────────────────────────

const touchCapable =
  window.matchMedia?.("(pointer: coarse)").matches || navigator.maxTouchPoints > 0;

if (touchCapable) {
  document.body.classList.add("touch");
  setupTouchPad();
}

function setupTouchPad() {
  const dpad = $("#dpad");
  const ARMS = { Up: ".dp-up", Down: ".dp-down", Left: ".dp-left", Right: ".dp-right" };
  // pointerId → 该指当前压住的方向集合;多指取并集后与键盘状态合并
  const active = new Map();
  let touchUnion = new Set();

  // 坐标 → 8 向方向集合(±67.5° 扇区,中心死区)
  function zones(x, y) {
    const r = dpad.getBoundingClientRect();
    const dx = x - (r.left + r.width / 2);
    const dy = y - (r.top + r.height / 2);
    const len = Math.hypot(dx, dy);
    const set = new Set();
    if (len < Math.min(r.width, r.height) * 0.14) return set;
    const T = 0.3827; // cos 67.5°
    const ux = dx / len;
    const uy = dy / len;
    if (ux > T) set.add("Right");
    if (ux < -T) set.add("Left");
    if (uy > T) set.add("Down");
    if (uy < -T) set.add("Up");
    return set;
  }

  function syncPressed() {
    const union = new Set();
    for (const set of active.values()) for (const d of set) union.add(d);
    for (const d of union) if (!touchUnion.has(d)) pressed.add(d);
    for (const d of touchUnion) if (!union.has(d)) pressed.delete(d);
    touchUnion = union;
    for (const [name, sel] of Object.entries(ARMS)) {
      $(sel).classList.toggle("active", union.has(name));
    }
  }

  dpad.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    dpad.setPointerCapture(e.pointerId);
    active.set(e.pointerId, zones(e.clientX, e.clientY));
    syncPressed();
  });
  dpad.addEventListener("pointermove", (e) => {
    if (!active.has(e.pointerId)) return;
    active.set(e.pointerId, zones(e.clientX, e.clientY));
    syncPressed();
  });
  const release = (e) => {
    if (!active.delete(e.pointerId)) return;
    syncPressed();
  };
  dpad.addEventListener("pointerup", release);
  dpad.addEventListener("pointercancel", release);
  dpad.addEventListener("lostpointercapture", release);
  dpad.addEventListener("contextmenu", (e) => e.preventDefault());

  // A / B / START / SELECT:按住即按住(指针捕获,滑出也可靠释放)
  function bindHold(el, name) {
    el.addEventListener("pointerdown", (e) => {
      e.preventDefault();
      el.setPointerCapture(e.pointerId);
      pressed.add(name);
      el.classList.add("active");
    });
    const off = () => {
      pressed.delete(name);
      el.classList.remove("active");
    };
    el.addEventListener("pointerup", off);
    el.addEventListener("pointercancel", off);
    el.addEventListener("lostpointercapture", off);
    el.addEventListener("contextmenu", (e) => e.preventDefault());
  }
  bindHold($("#padB"), "B");
  bindHold($("#padA"), "A");
  bindHold($("#padStart"), "Start");
  bindHold($("#padSelect"), "Select");
}

// ── 主循环 ──────────────────────────────────────────────

function drawNoise() {
  const data = imageData.data;
  for (let i = 0; i < data.length; i += 4) {
    // 冷色噪点,更像老电视
    const v = (Math.random() * 40) | 0;
    data[i] = v >> 1;
    data[i + 1] = v;
    data[i + 2] = v >> 1;
    data[i + 3] = 255;
  }
  offCtx.putImageData(imageData, 0, 0);
  ctx2d.drawImage(off, 0, 0, canvas.width, canvas.height);
}

function blitFrame() {
  imageData.data.set(nes.frame_rgba());
  offCtx.putImageData(imageData, 0, 0);
  ctx2d.drawImage(off, 0, 0, canvas.width, canvas.height);
}

let lastNoise = 0;

function tick(now) {
  requestAnimationFrame(tick);

  if (!romLoaded) {
    if (now - lastNoise > 80) { drawNoise(); lastNoise = now; } // ~12fps 噪点
    return;
  }

  if (rewinding) {
    rewindStep();
    return;
  }

  if (!paused) {
    // 按音频队列水位补帧:队列不足时运行模拟
    const target = QUEUE_TARGET * samplesPerFrame;
    let guard = 12; // 单次最多补 12 帧,防御死循环
    while (queued < target && guard-- > 0) {
      nes.run_frame();
      pushSamples(nes.audio_samples());
      nes.clear_audio_samples();
      fpsFrames++;
      // 每帧快照供倒带(失败不影响游戏)
      try {
        rewindBuf.push(nes.save_state());
        if (rewindBuf.length > REWIND_MAX_FRAMES) rewindBuf.shift();
      } catch { /* ignore */ }
    }

    const bits = currentBits();
    if (bits !== lastBits) {
      nes.set_controller_bits(0, bits);
      lastBits = bits;
    }

    blitFrame();

    if (now - fpsTime >= 500) {
      $("#stFps").textContent = `FPS ${Math.round(fpsFrames * 1000 / (now - fpsTime))}`;
      $("#stFrame").textContent = `FRAME ${nes.frame_number()}`;
      fpsFrames = 0;
      fpsTime = now;
    }
  }
}

// ── 倒带 ────────────────────────────────────────────

function startRewind() {
  if (!romLoaded || rewinding || rewindBuf.length === 0) return;
  rewinding = true;
  $("#btnRewind").setAttribute("aria-pressed", "true");
  $("#stFps").textContent = "◀◀ REWIND";
}

function stopRewind() {
  if (!rewinding) return;
  rewinding = false;
  $("#btnRewind").setAttribute("aria-pressed", "false");
  fpsTime = 0; // 下个 tick 重置 FPS 统计
}

function rewindStep() {
  if (rewindBuf.length === 0) { stopRewind(); return; }
  let state = null;
  for (let i = 0; i < REWIND_SPEED && rewindBuf.length > 0; i++) {
    state = rewindBuf.pop();
  }
  try {
    nes.load_state(state);
    clearAudioQueue();
    blitFrame();
    $("#stFrame").textContent = `FRAME ${nes.frame_number()}`;
  } catch (err) {
    stopRewind();
    toast(`倒带失败:${err.message || err}`, true);
  }
}

// ── ROM 装载 ────────────────────────────────────────────

async function insertCartridge(bytes, label, listItem = null) {
  ensureAudio(); // 用户手势内创建/恢复音频
  try {
    nes.load_rom(bytes);
    // 换卡即整机重启:清 CPU RAM 并复位,避免沿用旧游戏的脏 VRAM/寄存器导致花屏
    nes.power_cycle();
  } catch (err) {
    toast(`载入失败:${err.message || err}`, true);
    return;
  }
  romKey = nes.rom_md5_hex();
  romLoaded = true;
  paused = false;
  lastBits = 0;
  stopRewind();
  rewindBuf.length = 0;
  clearAudioQueue();

  // UI:遮罩掀开、LED 亮起、开机闪现
  $("#offOverlay").classList.add("hidden");
  const led = $("#led");
  led.classList.add("on");
  const screen = $(".crt-screen");
  screen.classList.remove("power-on");
  void screen.offsetWidth; // 重置动画
  screen.classList.add("power-on");

  document.querySelectorAll(".cart").forEach((el) => el.classList.remove("active"));
  if (listItem) listItem.classList.add("active");

  const short = romKey ? romKey.slice(0, 8) : "";
  $("#stMd5").textContent = short;
  $("#stFrame").textContent = "FRAME 0";
  $("#stFps").textContent = "FPS --";

  for (const id of ["btnPause", "btnReset", "btnRewind", "btnSave", "btnLoad"]) $(`#${id}`).disabled = false;
  $("#btnPause").textContent = "暂停";
  $("#btnPause").setAttribute("aria-pressed", "false");

  toast(`已插入:${label}`);
}

async function fetchRomList() {
  const list = $("#cartList");
  const hint = $("#shelfHint");
  let roms;
  try {
    const res = await fetch("roms/roms.json");
    if (!res.ok) throw new Error();
    roms = await res.json();
  } catch {
    hint.textContent = "未找到 roms/roms.json — 在仓库根目录运行 ./web/build.sh 生成";
    return;
  }
  if (roms.length === 0) {
    hint.textContent = "卡带架空空如也 — 把 .nes 文件拖进页面即可开玩";
    return;
  }
  for (const entry of roms) {
    const li = document.createElement("li");
    li.className = "cart";
    li.innerHTML = `<span><span class="cart-name"></span><span class="cart-file"></span></span>`;
    li.querySelector(".cart-name").textContent = prettify(entry.file);
    li.querySelector(".cart-file").textContent = `${entry.file} · ${(entry.size / 1024) | 0}KB`;
    li.addEventListener("click", async () => {
      if (li.classList.contains("active")) return;
      try {
        const res = await fetch(`roms/${encodeURIComponent(entry.file)}`);
        if (!res.ok) throw new Error(`HTTP ${res.status}`);
        const bytes = new Uint8Array(await res.arrayBuffer());
        await insertCartridge(bytes, prettify(entry.file), li);
      } catch (err) {
        toast(`读取失败:${err.message}`, true);
      }
    });
    list.appendChild(li);
  }
}

// 本地文件
$("#fileInput").addEventListener("change", async (e) => {
  const file = e.target.files[0];
  if (!file) return;
  const bytes = new Uint8Array(await file.arrayBuffer());
  const li = document.createElement("li");
  li.className = "cart custom";
  li.innerHTML = `<span><span class="cart-name"></span><span class="cart-file"></span></span>`;
  li.querySelector(".cart-name").textContent = prettify(file.name);
  li.querySelector(".cart-file").textContent = file.name;
  $("#cartList").prepend(li);
  await insertCartridge(bytes, prettify(file.name), li);
  e.target.value = "";
});

// 拖放
window.addEventListener("dragover", (e) => { e.preventDefault(); document.body.classList.add("dragging"); });
window.addEventListener("dragleave", (e) => {
  if (e.relatedTarget === null) document.body.classList.remove("dragging");
});
window.addEventListener("drop", async (e) => {
  e.preventDefault();
  document.body.classList.remove("dragging");
  const file = e.dataTransfer.files[0];
  if (!file || !/\.nes$/i.test(file.name)) { toast("只支持 .nes 文件", true); return; }
  const bytes = new Uint8Array(await file.arrayBuffer());
  const li = document.createElement("li");
  li.className = "cart custom";
  li.innerHTML = `<span><span class="cart-name"></span><span class="cart-file"></span></span>`;
  li.querySelector(".cart-name").textContent = prettify(file.name);
  li.querySelector(".cart-file").textContent = file.name;
  $("#cartList").prepend(li);
  await insertCartridge(bytes, prettify(file.name), li);
});

// ── 控制 ────────────────────────────────────────────────

function togglePause() {
  if (!romLoaded) return;
  paused = !paused;
  $("#btnPause").textContent = paused ? "继续" : "暂停";
  $("#btnPause").setAttribute("aria-pressed", String(paused));
  if (audioCtx) (paused ? audioCtx.suspend() : audioCtx.resume());
  toast(paused ? "已暂停" : "继续");
}

$("#btnPause").addEventListener("click", togglePause);

$("#btnReset").addEventListener("click", () => {
  if (!romLoaded) return;
  nes.reset();
  clearAudioQueue();
  $("#stFrame").textContent = "FRAME 0";
  toast("已复位");
});

$("#btnSave").addEventListener("click", () => {
  if (!romLoaded) return;
  try {
    const state = nes.save_state();
    // Uint8Array → base64
    let bin = "";
    const step = 0x8000;
    for (let i = 0; i < state.length; i += step) {
      bin += String.fromCharCode.apply(null, state.subarray(i, i + step));
    }
    localStorage.setItem(`nes-state:${romKey}`, btoa(bin));
    toast(`已存档(${(state.length / 1024) | 0}KB)`);
  } catch (err) {
    toast(`存档失败:${err.message || err}`, true);
  }
});

$("#btnLoad").addEventListener("click", () => {
  if (!romLoaded) return;
  const b64 = localStorage.getItem(`nes-state:${romKey}`);
  if (!b64) { toast("没有找到存档", true); return; }
  try {
    const bin = atob(b64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    nes.load_state(bytes);
    clearAudioQueue();
    blitFrame();
    toast("已读档");
  } catch (err) {
    toast(`读档失败:${err.message || err}`, true);
  }
});

$("#btnMute").addEventListener("click", () => {
  muted = !muted;
  $("#btnMute").setAttribute("aria-pressed", String(muted));
  if (gainNode) gainNode.gain.value = muted ? 0 : 1;
  toast(muted ? "已静音" : "声音开启");
});

// 倒带按钮:按住即回退(指针事件兼容鼠标/触摸)
const btnRewind = $("#btnRewind");
btnRewind.addEventListener("pointerdown", (e) => { e.preventDefault(); startRewind(); });
for (const ev of ["pointerup", "pointerleave", "pointercancel"]) {
  btnRewind.addEventListener(ev, stopRewind);
}

// 全屏
$("#btnFull").addEventListener("click", async () => {
  try {
    if (document.fullscreenElement) {
      await document.exitFullscreen();
    } else {
      await document.documentElement.requestFullscreen();
    }
  } catch {
    toast("当前环境不支持全屏", true);
  }
});

// ── 启动 ────────────────────────────────────────────────

(async () => {
  await init();
  nes = new NesWasm();
  await fetchRomList();
  requestAnimationFrame(tick);
})();
