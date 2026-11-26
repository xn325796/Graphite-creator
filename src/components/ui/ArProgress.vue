<script setup>
import { computed } from 'vue'

const props = defineProps({
  value: { type: Number, default: 0 },
  label: String,
  small: Boolean,
})
const pct = computed(() => Math.round(Math.min(1, Math.max(0, props.value)) * 100))
</script>

<template>
  <div class="ar-progress" :class="{ 'ar-progress-sm': small }">
    <div v-if="label" class="ar-progress-head">
      <span class="ar-progress-label">{{ label }}</span>
      <span class="ar-progress-pct">{{ pct }}%</span>
    </div>
    <div class="ar-track" role="progressbar" :aria-valuenow="pct" aria-valuemin="0" aria-valuemax="100">
      <div class="ar-bar" :style="{ width: pct + '%' }"></div>
    </div>
  </div>
</template>

<style scoped>
.ar-progress { display: flex; flex-direction: column; gap: var(--sp-3); width: 100%; }
.ar-progress-head { display: flex; justify-content: space-between; font-size: var(--fs-xs); }
.ar-progress-label { color: var(--text-2); }
.ar-progress-pct { color: var(--accent); font-variant-numeric: tabular-nums; }
.ar-track {
  height: 6px;
  border-radius: var(--r-pill);
  background: var(--border);
  overflow: hidden;
}
.ar-progress-sm .ar-track { height: 4px; }
.ar-bar {
  height: 100%;
  border-radius: var(--r-pill);
  background: linear-gradient(90deg, var(--accent-strong), var(--accent));
  transition: width var(--dur-slow) var(--ease);
}
</style>
