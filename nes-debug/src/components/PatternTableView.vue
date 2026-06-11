<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { PatternTableData } from '../types'

const props = defineProps<{
    running: boolean;
    tick:    number;
}>()

const canvas0 = ref<HTMLCanvasElement | null>(null)
const canvas1 = ref<HTMLCanvasElement | null>(null)

function b64ToBytes(b64: string): Uint8Array {
    const bin = atob(b64)
    const len = bin.length
    const bytes = new Uint8Array(len)
    for (let i = 0; i < len; i++) bytes[i] = bin.charCodeAt(i)

    return bytes
}

async function fetchData() {
    if (!props.running) return
    try {
        const data = await invoke<PatternTableData>('get_pattern_tables')
        renderTable(canvas0.value, data.table0_b64, data.size)
        renderTable(canvas1.value, data.table1_b64, data.size)
    }
    catch {

        // ignore
    }
}

function renderTable(canvas: HTMLCanvasElement | null, pixelsB64: string, size: number) {
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    const imageData = ctx.createImageData(size, size)
    imageData.data.set(b64ToBytes(pixelsB64))
    ctx.putImageData(imageData, 0, 0)
}

watch(() => props.tick, fetchData)
watch(() => props.running, r => { if (r) fetchData() })
onMounted(() => { if (props.running) fetchData() })
</script>

<template>
  <div class="panel">
    <h3 class="panel-title">
      Pattern Tables
    </h3>
    <div class="space-y-1.5">
      <div>
        <div class="text-[10px] text-[#888] mb-0.5">
          Left $0000
        </div>
        <canvas
          ref="canvas0"
          :width="128"
          :height="128"
          class="pattern-canvas"
        />
      </div>
      <div>
        <div class="text-[10px] text-[#888] mb-0.5">
          Right $1000
        </div>
        <canvas
          ref="canvas1"
          :width="128"
          :height="128"
          class="pattern-canvas"
        />
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
  @apply text-xs font-bold text-[#4fc3f7] mb-1.5 uppercase tracking-wider;
}
.pattern-canvas {
  image-rendering: pixelated;
  /* image-rendering: crisp-edges; */
  width: 100%;
  height: auto;
  background: #181818;
  border: 1px solid #0f3460;
  border-radius: 2px;
}
</style>
