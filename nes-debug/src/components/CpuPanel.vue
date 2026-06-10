<script setup lang="ts">
import type { CpuInfo } from "../types";

defineProps<{
  cpu: CpuInfo | null;
}>();

function hex(value: number, width: number = 2): string {
  return "$" + value.toString(16).toUpperCase().padStart(width, "0");
}
</script>

<template>
  <div class="panel">
    <h3 class="panel-title">CPU</h3>
    <div v-if="cpu" class="grid grid-cols-2 gap-x-4 gap-y-0.5 text-xs">
      <div class="reg-row">
        <span class="reg-label">A</span>
        <span class="reg-value">{{ hex(cpu.a) }}</span>
      </div>
      <div class="reg-row">
        <span class="reg-label">X</span>
        <span class="reg-value">{{ hex(cpu.x) }}</span>
      </div>
      <div class="reg-row">
        <span class="reg-label">Y</span>
        <span class="reg-value">{{ hex(cpu.y) }}</span>
      </div>
      <div class="reg-row">
        <span class="reg-label">SP</span>
        <span class="reg-value">{{ hex(cpu.sp) }}</span>
      </div>
      <div class="reg-row col-span-2">
        <span class="reg-label">PC</span>
        <span class="reg-value text-[#4fc3f7]">{{ hex(cpu.pc, 4) }}</span>
      </div>
    </div>
    <div v-if="cpu" class="mt-2 text-xs">
      <div class="flex gap-1 font-mono">
        <span :class="cpu.status & 0x80 ? 'flag-on' : 'flag-off'">N</span>
        <span :class="cpu.status & 0x40 ? 'flag-on' : 'flag-off'">V</span>
        <span :class="cpu.status & 0x10 ? 'flag-on' : 'flag-off'">B</span>
        <span :class="cpu.status & 0x08 ? 'flag-on' : 'flag-off'">D</span>
        <span :class="cpu.status & 0x04 ? 'flag-on' : 'flag-off'">I</span>
        <span :class="cpu.status & 0x02 ? 'flag-on' : 'flag-off'">Z</span>
        <span :class="cpu.status & 0x01 ? 'flag-on' : 'flag-off'">C</span>
      </div>
      <div class="mt-1.5 text-[#888]">
        CLK: {{ cpu.clocks.toLocaleString() }}
      </div>
      <div class="text-[#888]">
        INS: {{ cpu.instruction_counter.toLocaleString() }}
      </div>
      <div class="flex gap-3 mt-1">
        <span :class="cpu.irq_pending ? 'text-[#ff6b6b]' : 'text-[#555]'">IRQ</span>
        <span :class="cpu.nmi_line ? 'text-[#ffd93d]' : 'text-[#555]'">NMI</span>
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
  @apply text-[#888] w-6;
}
.reg-value {
  @apply text-[#e0e0e0] font-mono;
}
.flag-on {
  @apply text-[#4fc3f7] bg-[#0f3460] px-1 rounded;
}
.flag-off {
  @apply text-[#333] bg-[#1a1a2e] px-1 rounded;
}
</style>
