<script setup lang="ts">
import { ref, watch, onMounted, onUnmounted } from "vue";
import { invoke } from "@tauri-apps/api/core";

const props = defineProps<{
  running: boolean;
  paused: boolean;
  tick: number;
}>();

const region = ref<"ram" | "vram" | "chr" | "oam" | "palette">("ram");
const data = ref<number[]>([]);
const offset = ref(0);
const bytesPerRow = 16;
const rows = 16;

const REFRESH_MS = 500;
let timer: ReturnType<typeof setInterval> | null = null;

const regionInfo: Record<string, { label: string; size: number }> = {
  ram: { label: "CPU RAM", size: 0x800 },
  vram: { label: "VRAM", size: 0x1000 },
  chr: { label: "CHR ROM", size: 0x2000 },
  oam: { label: "OAM", size: 0x100 },
  palette: { label: "Palette", size: 0x20 },
};

async function fetchData() {
  if (!props.running) return;
  try {
    const result = await invoke<number[]>(`read_${region.value}`);
    data.value = result;
  } catch {
    data.value = [];
  }
}

function startTimer() {
  stopTimer();
  if (props.running && !props.paused) {
    fetchData();
    timer = setInterval(fetchData, REFRESH_MS);
  }
}

function stopTimer() {
  if (timer !== null) {
    clearInterval(timer);
    timer = null;
  }
}

function hexByte(b: number): string {
  return b.toString(16).toUpperCase().padStart(2, "0");
}

function hexAddr(a: number): string {
  return a.toString(16).toUpperCase().padStart(4, "0");
}

watch(region, () => {
  offset.value = 0;
  fetchData();
});

watch(() => props.tick, () => {
  fetchData();
});

watch(() => props.paused, (p) => {
  if (p) {
    stopTimer();
    fetchData();
  } else {
    startTimer();
  }
});

watch(() => props.running, (r) => {
  if (r) startTimer();
  else stopTimer();
});

onMounted(() => {
  if (props.running && !props.paused) startTimer();
});

onUnmounted(stopTimer);

defineExpose({ fetchData });
</script>

<template>
  <div class="panel">
    <div class="flex items-center justify-between mb-1.5">
      <h3 class="panel-title mb-0">Memory</h3>
      <div class="flex gap-0.5">
        <button
          v-for="(_, key) in regionInfo"
          :key="key"
          :class="region === key ? 'tab-active' : 'tab-inactive'"
          @click="region = key as typeof region"
        >
          {{ regionInfo[key]?.label }}
        </button>
      </div>
    </div>
    <div class="font-mono text-[10px] leading-[14px] text-[#888] overflow-auto max-h-[320px]">
      <div class="flex text-[#4fc3f7] mb-0.5 sticky top-0 bg-[#16213e]">
        <span class="w-12 shrink-0">ADDR</span>
        <span v-for="i in bytesPerRow" :key="i" class="w-[18px] text-center">
          {{ (i - 1).toString(16).toUpperCase() }}
        </span>
        <span class="ml-1">ASCII</span>
      </div>
      <div v-for="r in Math.min(rows, Math.ceil(data.length / bytesPerRow) - Math.floor(offset / bytesPerRow))" :key="r">
        <div v-if="(r - 1) * bytesPerRow + offset < data.length" class="flex">
          <span class="w-12 shrink-0 text-[#4fc3f7]">{{ hexAddr((r - 1) * bytesPerRow + offset) }}</span>
          <span
            v-for="c in bytesPerRow"
            :key="c"
            class="w-[18px] text-center"
            :class="data[(r - 1) * bytesPerRow + (c - 1) + offset] !== undefined ? 'text-[#ccc]' : 'text-[#333]'"
          >
            {{ data[(r - 1) * bytesPerRow + (c - 1) + offset] !== undefined
              ? hexByte(data[(r - 1) * bytesPerRow + (c - 1) + offset]!) : ".." }}
          </span>
          <span class="ml-1 text-[#555]">
            <span v-for="c in bytesPerRow" :key="c">
              {{ (() => {
                const b = data[(r - 1) * bytesPerRow + (c - 1) + offset];
                return b !== undefined && b >= 0x20 && b < 0x7F ? String.fromCharCode(b) : '.';
              })() }}
            </span>
          </span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
@reference "tailwindcss";
.panel {
  @apply bg-[#16213e] rounded p-2 border border-[#0f3460];
}
.panel-title {
  @apply text-xs font-bold text-[#4fc3f7] uppercase tracking-wider;
}
.tab-active {
  @apply px-1.5 py-0.5 text-[10px] rounded bg-[#0f3460] text-[#4fc3f7] cursor-pointer;
}
.tab-inactive {
  @apply px-1.5 py-0.5 text-[10px] rounded bg-[#1a1a2e] text-[#555] hover:text-[#888] cursor-pointer;
}
</style>
