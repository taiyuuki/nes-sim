<script setup lang="ts">
import { ref } from "vue";

const emit = defineEmits<{
  add: [type: string, value?: number];
  remove: [type: string, value?: number];
}>();

const bpType = ref("address");
const bpValue = ref("");

const types = [
  { value: "address", label: "地址" },
  { value: "memory_read", label: "内存读" },
  { value: "memory_write", label: "内存写" },
  { value: "ppu_scanline", label: "PPU 行" },
  { value: "vblank", label: "VBlank" },
];

function addBreakpoint() {
  const val = bpValue.value.trim();
  if (bpType.value === "vblank") {
    emit("add", "vblank");
  } else if (val) {
    const num = parseInt(val, val.startsWith("$") ? 16 : 10);
    if (!isNaN(num)) {
      emit("add", bpType.value, num);
    }
  }
}
</script>

<template>
  <div class="panel">
    <h3 class="panel-title">断点</h3>
    <div class="flex gap-1 items-center mb-2">
      <select v-model="bpType" class="bp-select">
        <option v-for="t in types" :key="t.value" :value="t.value">{{ t.label }}</option>
      </select>
      <input
        v-model="bpValue"
        :disabled="bpType === 'vblank'"
        placeholder="$0000"
        class="bp-input"
        @keydown.enter="addBreakpoint"
      />
      <button class="bp-add-btn" @click="addBreakpoint">+</button>
    </div>
    <div class="text-[10px] text-[#555]">
      支持十进制或 $hex 格式
    </div>
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
.bp-select {
  @apply bg-[#0f3460] text-[#e0e0e0] text-[10px] rounded px-1 py-0.5 border-none outline-none;
}
.bp-input {
  @apply bg-[#0f3460] text-[#e0e0e0] text-[10px] rounded px-1.5 py-0.5 w-16 border border-[#1a4680]
    outline-none focus:border-[#4fc3f7] placeholder-[#555];
}
.bp-add-btn {
  @apply bg-[#0f3460] text-[#4fc3f7] text-xs rounded px-2 py-0.5 hover:bg-[#1a4680] cursor-pointer;
}
</style>
