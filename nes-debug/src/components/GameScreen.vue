<script setup lang="ts">
import { ref, watch, onMounted } from "vue";
import type { FrameData } from "../types";
import { NES_PALETTE } from "../palette";

const props = defineProps<{
  frame: FrameData | null;
}>();

const canvas = ref<HTMLCanvasElement | null>(null);

function renderFrame() {
  if (!canvas.value || !props.frame) return;
  const ctx = canvas.value.getContext("2d");
  if (!ctx) return;

  const { width, height, pixels } = props.frame;
  const imageData = ctx.createImageData(width, height);
  const data = imageData.data;

  for (let i = 0; i < pixels.length; i++) {
    const color = NES_PALETTE[pixels[i]!] ?? [0, 0, 0];
    const j = i * 4;
    data[j] = color[0];
    data[j + 1] = color[1];
    data[j + 2] = color[2];
    data[j + 3] = 255;
  }

  ctx.putImageData(imageData, 0, 0);
}

watch(() => props.frame, renderFrame);

onMounted(() => {
  renderFrame();
});
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
  image-rendering: crisp-edges;
}
</style>
