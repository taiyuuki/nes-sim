<script setup lang="ts">
import type { PpuInfo } from "../types";

defineProps<{
  ppu: PpuInfo | null;
}>();

function hex(value: number, width: number = 2): string {
  return "$" + value.toString(16).toUpperCase().padStart(width, "0");
}
</script>

<template>
  <div class="panel">
    <h3 class="panel-title">PPU</h3>
    <div v-if="ppu" class="grid grid-cols-2 gap-x-4 gap-y-0.5 text-xs">
      <div class="reg-row">
        <span class="reg-label">Frame</span>
        <span class="reg-value">{{ ppu.frame }}</span>
      </div>
      <div class="reg-row">
        <span class="reg-label">SL</span>
        <span class="reg-value">{{ ppu.scanline }}</span>
      </div>
      <div class="reg-row">
        <span class="reg-label">CYC</span>
        <span class="reg-value">{{ ppu.cycles }}</span>
      </div>
      <div class="reg-row">
        <span class="reg-label">OAM</span>
        <span class="reg-value">{{ hex(ppu.oam_addr) }}</span>
      </div>
      <div class="reg-row">
        <span class="reg-label">PPUADDR</span>
        <span class="reg-value text-[#4fc3f7]">{{ hex(ppu.vram_addr, 4) }}</span>
      </div>
      <div class="reg-row">
        <span class="reg-label">TMP</span>
        <span class="reg-value">{{ hex(ppu.temp_vram_addr, 4) }}</span>
      </div>
    </div>
    <div v-if="ppu" class="mt-2 space-y-0.5 text-xs">
      <div class="flex gap-3">
        <span class="reg-label w-8">CTRL</span>
        <span class="reg-value font-mono">{{ hex(ppu.ctrl) }}</span>
      </div>
      <div class="flex gap-3">
        <span class="reg-label w-8">MASK</span>
        <span class="reg-value font-mono">{{ hex(ppu.mask) }}</span>
      </div>
      <div class="flex gap-3">
        <span class="reg-label w-8">STAT</span>
        <span class="reg-value font-mono">{{ hex(ppu.status) }}</span>
      </div>
      <div class="flex gap-2 mt-1.5">
        <span :class="ppu.bg_on ? 'badge-on' : 'badge-off'">BG</span>
        <span :class="ppu.sprites_on ? 'badge-on' : 'badge-off'">OBJ</span>
        <span :class="ppu.in_vblank ? 'badge-on' : 'badge-off'">VB</span>
        <span :class="ppu.nmi_line ? 'badge-on' : 'badge-off'">NMI</span>
      </div>
    </div>
    <div v-else class="text-[#666] text-xs">No data</div>
  </div>
</template>

<style scoped>
@reference "tailwindcss";
.panel {
  @apply bg-[#16213e] rounded p-2 border border-[#0f3460];
}
.panel-title {
  @apply text-xs font-bold text-[#4fc3f7] mb-1.5 uppercase tracking-wider;
}
.reg-row {
  @apply flex justify-between;
}
.reg-label {
  @apply text-[#888];
}
.reg-value {
  @apply text-[#e0e0e0];
}
.badge-on {
  @apply text-[#4fc3f7] bg-[#0f3460] px-1.5 py-0.5 rounded text-[10px];
}
.badge-off {
  @apply text-[#333] bg-[#1a1a2e] px-1.5 py-0.5 rounded text-[10px];
}
</style>
