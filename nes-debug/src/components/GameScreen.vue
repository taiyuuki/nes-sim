<script setup lang="ts">
import { onMounted, ref, watch } from 'vue'
import type { FrameData } from '../types'
import { PALETTE_LUT } from '../palette'

const props = defineProps<{ frame: FrameData | null; }>()

const canvas = ref<HTMLCanvasElement | null>(null)

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
    const data = imageData.data
    const len = pixels.length
    for (let i = 0; i < len; i++) {
        const offset = (pixels[i]! & 0x3F) << 2
        const j = i << 2
        data[j] = PALETTE_LUT[offset]
        data[j + 1] = PALETTE_LUT[offset + 1]
        data[j + 2] = PALETTE_LUT[offset + 2]
        data[j + 3] = 255
    }
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
