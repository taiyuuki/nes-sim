<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { NametableData } from '../types'

const props = defineProps<{
    running: boolean;
    tick:    number;
}>()

const canvas = ref<HTMLCanvasElement | null>(null)

const selectedTable = ref(0)
const enabled = ref(false)

onMounted(() => {
    const saved = localStorage.getItem('nametable-enabled')
    if (saved !== null) {
        enabled.value = saved === 'true'
    }
})

watch(enabled, value => {
    localStorage.setItem('nametable-enabled', String(value))
    if (value && props.running) {
        fetchData()
    }
})

function b64ToBytes(b64: string): Uint8Array {
    const bin = atob(b64)
    const len = bin.length
    const bytes = new Uint8Array(len)
    for (let i = 0; i < len; i++) bytes[i] = bin.charCodeAt(i)

    return bytes
}

async function fetchData() {
    if (!props.running || !enabled.value) return
    try {
        const data = await invoke<NametableData>('get_nametable', { tableIndex: selectedTable.value })
        if (!canvas.value) return
        const ctx = canvas.value.getContext('2d')
        if (!ctx) return
        const imageData = ctx.createImageData(data.width, data.height)
        imageData.data.set(b64ToBytes(data.pixels_b64))
        ctx.putImageData(imageData, 0, 0)
    }
    catch {

        // ignore
    }
}

watch(() => props.tick, fetchData)
watch(() => props.running, r => { if (r && enabled.value) fetchData() })
watch(selectedTable, () => { if (enabled.value) fetchData() })
onMounted(() => { if (props.running && enabled.value) fetchData() })

const tableNames = ['$2000', '$2400', '$2800', '$2C00']
</script>

<template>
  <div class="panel">
    <div class="flex items-center justify-between mb-1.5">
      <h3 class="panel-title mb-0">
        Nametables
      </h3>
      <button
        :class="['toggle-btn', enabled ? 'toggle-btn-on' : 'toggle-btn-off']"
        title="启用/禁用 Nametable 渲染"
        @click="enabled = !enabled"
      >
        {{ enabled ? 'ON' : 'OFF' }}
      </button>
    </div>

    <div
      v-show="enabled"
      class="table-selector mb-2"
    >
      <button
        v-for="(name, idx) in tableNames"
        :key="idx"
        :class="['table-btn', selectedTable === idx ? 'table-btn-active' : '']"
        @click="selectedTable = idx"
      >
        {{ name }}
      </button>
    </div>

    <div
      v-show="enabled"
      class="nametable-container"
    >
      <canvas
        ref="canvas"
        :width="256"
        :height="240"
        class="nametable-canvas"
      />
    </div>

    <div
      v-show="!enabled"
      class="text-[#888] text-xs py-2 text-center"
    >
      Nametable 渲染已禁用以提升性能
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
.toggle-btn {
  @apply text-[10px] font-bold rounded py-0.5 px-2 transition-colors;
}
.toggle-btn-on {
  @apply text-[#4fc3f7] bg-[#0f3460];
}
.toggle-btn-off {
  @apply text-[#666] bg-[#1a1a2e] hover:bg-[#252540];
}
.table-selector {
  @apply flex gap-1;
}
.table-btn {
  @apply flex-1 text-[10px] text-[#888] bg-[#1a1a2e] rounded py-1 px-2 hover:bg-[#0f3460] hover:text-[#e0e0e0] transition-colors;
}
.table-btn-active {
  @apply text-[#4fc3f7] bg-[#0f3460];
}
.nametable-container {
  @apply relative;
}
.nametable-canvas {
  image-rendering: pixelated;
  image-rendering: crisp-edges;
  width: 100%;
  height: auto;
  background: #181818;
  border: 1px solid #0f3460;
  border-radius: 2px;
}
</style>
