<script setup lang="ts">
import { useAudioPlayer } from '../composables/useAudioPlayer'

const audio = useAudioPlayer()

function formatVolume(value: number): string {
    return `${Math.round(value * 100)}%`
}

function onVolumeChange(event: Event) {
    const target = event.target as HTMLInputElement
    audio.setVolume(target.valueAsNumber / 100)
}
</script>

<template>
  <div class="panel">
    <div class="flex items-center justify-between mb-2">
      <h3 class="panel-title mb-0">
        Audio
      </h3>
      <button
        :class="['toggle-btn', audio.enabled ? 'toggle-btn-on' : 'toggle-btn-off']"
        :title="audio.enabled ? '禁用音频' : '启用音频'"
        @click="audio.toggle()"
      >
        {{ audio.enabled ? 'ON' : 'OFF' }}
      </button>
    </div>

    <div
      v-show="audio.enabled"
      class="space-y-2"
    >
      <div class="flex items-center gap-2">
        <span class="text-[10px] text-[#888] w-8">
          音量
        </span>
        <input
          type="range"
          min="0"
          max="100"
          :value="audio.volume.value * 100"
          class="volume-slider flex-1"
          @input="onVolumeChange"
        >
        <span class="text-[10px] text-[#e0e0e0] w-10 text-right">
          {{ formatVolume(audio.volume.value) }}
        </span>
      </div>

      <div class="flex items-center gap-2 text-[10px]">
        <span class="text-[#888]">状态:</span>
        <span :class="audio.isPlaying ? 'text-green-400' : 'text-[#666]'">
          {{ audio.isPlaying ? '播放中' : '待机' }}
        </span>
      </div>
    </div>

    <div
      v-show="!audio.enabled"
      class="text-[#888] text-xs py-2 text-center"
    >
      音频已禁用
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
.volume-slider {
  @apply appearance-none bg-[#1a1a2e] rounded-full h-1.5 cursor-pointer;
}
.volume-slider::-webkit-slider-thumb {
  @apply appearance-none w-3 h-3 rounded-full bg-[#4fc3f7] cursor-pointer;
}
.volume-slider::-moz-range-thumb {
  @apply w-3 h-3 rounded-full bg-[#4fc3f7] cursor-pointer border-0;
}
</style>
