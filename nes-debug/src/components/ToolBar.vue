<script setup lang="ts">
import { open } from "@tauri-apps/plugin-dialog";

const emit = defineEmits<{
  loadRom: [path: string];
  reset: [];
  stepFrame: [];
  stepInstruction: [];
  togglePause: [];
}>();

async function openRom() {
  const path = await open({
    filters: [{ name: "NES ROM", extensions: ["nes"] }],
  });
  if (path) {
    emit("loadRom", path);
  }
}
</script>

<template>
  <div class="flex items-center gap-1 bg-[#16213e] px-3 py-1.5 border-b border-[#0f3460]">
    <button
      class="toolbar-btn"
      @click="openRom"
    >
      打开 ROM
    </button>
    <div class="w-px h-5 bg-[#0f3460] mx-1" />
    <button class="toolbar-btn" @click="$emit('reset')">重置</button>
    <button class="toolbar-btn" @click="$emit('togglePause')">暂停/继续</button>
    <button class="toolbar-btn" @click="$emit('stepFrame')">逐帧</button>
    <button class="toolbar-btn" @click="$emit('stepInstruction')">逐指令</button>
  </div>
</template>

<style scoped>
@reference "tailwindcss";
.toolbar-btn {
  @apply px-2.5 py-1 text-xs rounded bg-[#0f3460] text-[#e0e0e0] hover:bg-[#1a4680]
    disabled:opacity-40 disabled:cursor-not-allowed transition-colors cursor-pointer;
}
</style>
