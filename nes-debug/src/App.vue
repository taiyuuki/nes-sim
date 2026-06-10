<script setup lang="ts">
import { useEmulator } from "./composables/useEmulator";
import ToolBar from "./components/ToolBar.vue";
import GameScreen from "./components/GameScreen.vue";
import DisassemblyView from "./components/DisassemblyView.vue";
import CpuPanel from "./components/CpuPanel.vue";
import PpuPanel from "./components/PpuPanel.vue";
import MemoryView from "./components/MemoryView.vue";
import BreakpointPanel from "./components/BreakpointPanel.vue";
import StatusPanel from "./components/StatusPanel.vue";
import PatternTableView from "./components/PatternTableView.vue";

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
      <!-- 左侧：游戏画面 + 反汇编 + 内存 -->
      <div class="flex-1 flex flex-col min-w-0">
        <GameScreen :frame="emu.frameData.value" />
        <div class="flex flex-1 gap-2 m-2 min-h-0">
          <DisassemblyView :result="emu.disasmResult.value" class="flex-1 min-w-0" />
          <MemoryView :running="emu.running.value" :paused="emu.paused.value" :tick="emu.tick.value" class="flex-1 min-w-0" />
        </div>
      </div>

      <!-- 右侧：Debug 面板 -->
      <div class="w-80 shrink-0 overflow-y-auto p-2 space-y-2 border-l border-[#0f3460] bg-[#1a1a2e]">
        <StatusPanel
          :info="emu.debugInfo.value"
          :rom-path="emu.romPath.value"
          :error="emu.error.value"
        />
        <CpuPanel :cpu="emu.debugInfo.value?.cpu ?? null" />
        <PpuPanel :ppu="emu.debugInfo.value?.ppu ?? null" />
        <PatternTableView :running="emu.running.value" :tick="emu.tick.value" />
        <BreakpointPanel
          @add="emu.addBreakpoint"
          @remove="emu.removeBreakpoint"
        />
      </div>
    </div>
  </div>
</template>
