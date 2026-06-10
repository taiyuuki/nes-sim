<script setup lang="ts">
import type { DebugInfo } from "../types";

defineProps<{
  info: DebugInfo | null;
  romPath: string;
  error: string;
}>();
</script>

<template>
  <div class="panel">
    <h3 class="panel-title">状态</h3>
    <div class="text-xs space-y-0.5">
      <div v-if="romPath" class="text-[#888] truncate" :title="romPath">
        ROM: {{ romPath.split("/").pop() ?? romPath.split("\\").pop() ?? romPath }}
      </div>
      <div v-if="info" class="flex gap-2">
        <span :class="info.paused ? 'text-[#ffd93d]' : 'text-[#4fc3f7]'">
          {{ info.paused ? "已暂停" : "运行中" }}
        </span>
        <span class="text-[#555]">
          帧 #{{ info.frame_number }}
        </span>
      </div>
      <div v-if="info" class="text-[#555]">
        MCLK: {{ info.master_clock.toLocaleString() }}
      </div>
      <div v-if="error" class="text-[#ff6b6b] text-[10px] break-all">
        {{ error }}
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
</style>
