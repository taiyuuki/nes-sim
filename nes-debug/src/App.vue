<script setup lang="ts">
import { useEmulator } from "./composables/useEmulator";
import ToolBar from "./components/ToolBar.vue";
import GameScreen from "./components/GameScreen.vue";
import CpuPanel from "./components/CpuPanel.vue";
import PpuPanel from "./components/PpuPanel.vue";
import MemoryView from "./components/MemoryView.vue";
import BreakpointPanel from "./components/BreakpointPanel.vue";
import StatusPanel from "./components/StatusPanel.vue";

const emu = useEmulator();

async function onToolbarAction(action: string) {
  switch (action) {
    case "reset":
      await emu.reset();
      break;
    case "togglePause":
      await emu.togglePause();
      break;
    case "stepFrame":
      await emu.stepFrame();
      break;
    case "stepInstruction":
      await emu.stepInstruction();
      break;
  }
}

function onToolbarEvent(event: string, payload?: string) {
  if (event === "loadRom" && payload) {
    emu.loadRom(payload);
  } else {
    onToolbarAction(event);
  }
}
</script>

<template>
  <div class="h-screen flex flex-col bg-[#1a1a2e] text-[#e0e0e0]">
    <ToolBar
      @load-rom="(p: string) => onToolbarEvent('loadRom', p)"
      @reset="onToolbarAction('reset')"
      @toggle-pause="onToolbarAction('togglePause')"
      @step-frame="onToolbarAction('stepFrame')"
      @step-instruction="onToolbarAction('stepInstruction')"
    />

    <div class="flex flex-1 overflow-hidden">
      <!-- 左侧：游戏画面 -->
      <div class="flex-1 flex flex-col min-w-0">
        <GameScreen :frame="emu.frameData.value" />
        <MemoryView :running="emu.running.value" :paused="emu.paused.value" :tick="emu.tick.value" class="flex-1 m-2" />
      </div>

      <!-- 右侧：Debug 面板 -->
      <div class="w-72 shrink-0 overflow-y-auto p-2 space-y-2 border-l border-[#0f3460] bg-[#1a1a2e]">
        <StatusPanel
          :info="emu.debugInfo.value"
          :rom-path="emu.romPath.value"
          :error="emu.error.value"
        />
        <CpuPanel :cpu="emu.debugInfo.value?.cpu ?? null" />
        <PpuPanel :ppu="emu.debugInfo.value?.ppu ?? null" />
        <BreakpointPanel
          @add="emu.addBreakpoint"
          @remove="emu.removeBreakpoint"
        />
      </div>
    </div>
  </div>
</template>
