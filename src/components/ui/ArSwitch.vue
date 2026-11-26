<script setup>
const model = defineModel({ type: Boolean, default: false })
defineProps({
  label: String,
  disabled: Boolean,
  size: { type: String, default: 'md' },
})
</script>

<template>
  <label class="ar-switch" :class="[`ar-switch-${size}`, { on: model, disabled }]">
    <input v-model="model" type="checkbox" class="ar-switch-input" :disabled="disabled" />
    <span class="ar-track"><span class="ar-knob"></span></span>
    <span v-if="label" class="ar-switch-label">{{ label }}</span>
  </label>
</template>

<style scoped>
.ar-switch {
  display: inline-flex;
  align-items: center;
  gap: var(--sp-3);
  cursor: pointer;
  user-select: none;
}
.ar-switch.disabled { opacity: 0.4; cursor: default; }
.ar-switch-input { position: absolute; opacity: 0; pointer-events: none; }

.ar-track {
  position: relative;
  flex: none;
  width: 32px;
  height: 18px;
  border-radius: var(--r-pill);
  background: var(--border-strong);
  transition: background var(--dur) var(--ease);
}
.ar-switch-sm .ar-track { width: 28px; height: 16px; }

.ar-knob {
  position: absolute;
  left: 2px;
  top: 2px;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: #d6dde3;
  transition: left var(--dur) var(--ease), background var(--dur) var(--ease);
}
.ar-switch-sm .ar-knob { width: 12px; height: 12px; }

.ar-switch.on .ar-track { background: var(--accent-strong); }
.ar-switch.on .ar-knob { left: 16px; background: var(--accent-ink); }
.ar-switch-sm.on .ar-knob { left: 14px; }

.ar-switch-label { color: var(--text); font-size: var(--fs-sm); }
.ar-switch:focus-within .ar-track { box-shadow: var(--ring); }
</style>
