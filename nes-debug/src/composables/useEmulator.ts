import { ref, shallowRef } from "vue";
import { invoke } from "@tauri-apps/api/core";
import type { DebugInfo, FrameData } from "../types";

const debugInfo = shallowRef<DebugInfo | null>(null);
const frameData = shallowRef<FrameData | null>(null);
const paused = ref(true);
const running = ref(false);
const romPath = ref("");
const error = ref("");
const tick = ref(0);

let loopId = 0;

export function useEmulator() {
  async function loadRom(path: string) {
    try {
      error.value = "";
      await invoke("load_rom", { path });
      romPath.value = path;
      running.value = true;
      await invoke("set_paused", { paused: true });
      paused.value = true;
      await refresh();
    } catch (e) {
      error.value = String(e);
    }
  }

  async function reset() {
    try {
      await invoke("reset");
      await refresh();
    } catch (e) {
      error.value = String(e);
    }
  }

  async function stepFrame() {
    if (!paused.value) return;
    try {
      const info = await invoke<DebugInfo>("step_frame");
      debugInfo.value = info;
      await fetchFrame();
      tick.value++;
    } catch (e) {
      error.value = String(e);
    }
  }

  async function stepInstruction() {
    if (!paused.value) return;
    try {
      const info = await invoke<DebugInfo>("step_instruction");
      debugInfo.value = info;
      await fetchFrame();
      tick.value++;
    } catch (e) {
      error.value = String(e);
    }
  }

  function togglePause() {
    if (paused.value) {
      resume();
    } else {
      pause();
    }
  }

  async function pause() {
    stopLoop();
    paused.value = true;
    await invoke("set_paused", { paused: true }).catch(() => {});
    await refresh();
  }

  async function resume() {
    paused.value = false;
    await invoke("set_paused", { paused: false }).catch(() => {});
    startLoop();
  }

  function startLoop() {
    stopLoop();
    const id = ++loopId;
    let lastTick = performance.now();
    const loop = async () => {
      if (loopId !== id || paused.value) return;
      try {
        const info = await invoke<DebugInfo>("run_frame");
        debugInfo.value = info;
        const frame = await invoke<FrameData>("get_frame");
        frameData.value = frame;
        const now = performance.now();
        if (now - lastTick >= 500) {
          tick.value++;
          lastTick = now;
        }
        if (info.paused) {
          paused.value = true;
          tick.value++;
          return;
        }
        requestAnimationFrame(loop);
      } catch (e) {
        error.value = String(e);
        paused.value = true;
      }
    };
    requestAnimationFrame(loop);
  }

  function stopLoop() {
    loopId++;
  }

  async function addBreakpoint(type: string, value?: number) {
    try {
      await invoke("add_breakpoint", { bpDef: { type, value } });
    } catch (e) {
      error.value = String(e);
    }
  }

  async function removeBreakpoint(type: string, value?: number) {
    try {
      await invoke("remove_breakpoint", { bpDef: { type, value } });
    } catch (e) {
      error.value = String(e);
    }
  }

  async function refresh() {
    try {
      const info = await invoke<DebugInfo>("get_debug_info");
      debugInfo.value = info;
      await fetchFrame();
    } catch (e) {
      error.value = String(e);
    }
  }

  async function fetchFrame() {
    try {
      const frame = await invoke<FrameData>("get_frame");
      frameData.value = frame;
    } catch (e) {
      error.value = String(e);
    }
  }

  return {
    debugInfo,
    frameData,
    paused,
    running,
    romPath,
    error,
    tick,
    loadRom,
    reset,
    stepFrame,
    stepInstruction,
    togglePause,
    pause,
    resume,
    addBreakpoint,
    removeBreakpoint,
    refresh,
  };
}
