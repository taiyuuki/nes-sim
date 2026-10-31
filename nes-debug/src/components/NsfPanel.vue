<script setup lang="ts">
import type { NsfInfo } from '../types'

const props = defineProps<{
    info: NsfInfo | null;
}>()

const emit = defineEmits<{
    (e: 'select-track', track: number): void;
}>()

function step(delta: number) {
    if (!props.info) return
    const count = props.info.track_count
    let next = props.info.current_track + delta
    if (next < 1) next = count
    if (next > count) next = 1
    emit('select-track', next)
}
</script>

<template>
  <div
    v-if="info"
    class="panel"
  >
    <h3 class="panel-title">
      NSF 播放器
    </h3>
    <div class="text-xs space-y-1">
      <div
        class="text-[#ffd93d] truncate"
        :title="info.name"
      >
        ♪ {{ info.name || '<?>' }}
      </div>
      <div class="text-[#888] truncate">
        <template v-if="info.artist">
          {{ info.artist }}
        </template>
        <template v-if="info.artist && info.copyright">
          ·
        </template>
        <template v-if="info.copyright">
          {{ info.copyright }}
        </template>
      </div>
      <div class="flex items-center gap-2 pt-0.5">
        <button
          class="btn"
          title="上一曲 ([)"
          @click="step(-1)"
        >
          ◀
        </button>
        <span class="text-[#e0e0e0] tabular-nums">
          {{ info.current_track }} / {{ info.track_count }}
        </span>
        <button
          class="btn"
          title="下一曲 (])"
          @click="step(1)"
        >
          ▶
        </button>
      </div>
      <div
        v-if="info.chips.length"
        class="flex flex-wrap gap-1 pt-0.5"
      >
        <span
          v-for="chip in [info.tv_system, ...info.chips]"
          :key="chip"
          class="chip"
        >{{ chip }}</span>
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
.btn {
  @apply w-7 h-6 text-[10px] text-[#e0e0e0] bg-[#0f3460] border border-[#1a4a7a] rounded cursor-pointer;
}
.btn:hover {
  @apply border-[#4fc3f7] text-[#4fc3f7];
}
.chip {
  @apply text-[9px] px-1.5 py-0.5 border border-[#1a4a7a] rounded text-[#888];
}
</style>
