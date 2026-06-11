<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import type { FrameData } from '../types'
import { NES_LUT } from '../nes-lut'

const props = defineProps<{ frame: FrameData | null; }>()

const canvas = ref<HTMLCanvasElement | null>(null)

// 预计算 Uint32 查找表，利用小端序一次写入 4 字节
const LUT32 = new Uint32Array(64)
const lutView = new DataView(NES_LUT.buffer)
for (let i = 0; i < 64; i++) LUT32[i] = lutView.getUint32(i * 4, true)

function b64ToBytes(b64: string): Uint8Array {
    const bin = atob(b64)
    const len = bin.length
    const bytes = new Uint8Array(len)
    for (let i = 0; i < len; i++) bytes[i] = bin.charCodeAt(i)

    return bytes
}

function renderFrame() {
    if (!canvas.value || !props.frame) return
    const ctx = canvas.value.getContext('2d')
    if (!ctx) return

    const { width, height, pixels_b64 } = props.frame
    const pixels = b64ToBytes(pixels_b64)
    const imageData = ctx.createImageData(width, height)
    const buf = new Uint32Array(imageData.data.buffer)
    const len = pixels.length
    for (let i = 0; i < len; i++) buf[i] = LUT32[pixels[i]! & 0x3F]
    ctx.putImageData(imageData, 0, 0)
}

watch(() => props.frame, renderFrame)

onMounted(() => {
    renderFrame()
})
</script>

<template>
  <div class="flex items-center justify-center bg-black p-2">
    <canvas
      ref="canvas"
      :width="frame?.width ?? 256"
      :height="frame?.height ?? 240"
      class="block image-rendering-pixelated"
      style="image-rendering: pixelated;"
    />
  </div>
</template>

<style scoped>
canvas {
  width: 100%;
  max-width: 512px;
  height: auto;
  image-rendering: pixelated;
  /* image-rendering: crisp-edges; */
}
</style>
