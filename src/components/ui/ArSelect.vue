<script setup>
const model = defineModel({ type: String, default: '' })
defineProps({
  options: { type: Array, default: () => [] },
  label: String,
  invalid: Boolean,
  disabled: Boolean,
  size: { type: String, default: 'md' },
})
</script>

<template>
  <label class="ar-select" :class="[`ar-select-${size}`, { 'ar-select-invalid': invalid, 'ar-select-disabled': disabled }]">
    <select v-model="model" :disabled="disabled" :aria-label="label">
      <slot />
      <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
    </select>
    <span class="ar-chevron" aria-hidden="true">
      <svg viewBox="0 0 10 6"><path d="M1 1l4 4 4-4" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></svg>
    </span>
  </label>
</template>

<style scoped>
.ar-select {
  position: relative;
  display: inline-flex;
  align-items: center;
  height: var(--h-control-md);
  min-width: 96px;
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
  background: var(--bg-field);
  transition: border-color var(--dur-fast) var(--ease), box-shadow var(--dur-fast) var(--ease);
}
.ar-select-sm { height: var(--h-control-sm); }
.ar-select:hover:not(.ar-select-disabled) { border-color: var(--border-strong); }
.ar-select:focus-within { border-color: var(--accent-strong); box-shadow: 0 0 0 3px var(--accent-soft); }
.ar-select-invalid { border-color: var(--danger); }
.ar-select-disabled { opacity: 0.5; }
.ar-select select {
  appearance: none;
  width: 100%;
  height: 100%;
  padding: 0 26px 0 var(--sp-4);
  border: 0;
  outline: none;
  background: transparent;
  color: var(--text);
  font-family: inherit;
  font-size: var(--fs-sm);
  cursor: pointer;
}
.ar-select-sm select { font-size: var(--fs-xs); padding-left: var(--sp-3); }
.ar-select select option { background: var(--bg-elev); color: var(--text); }
.ar-chevron {
  position: absolute;
  right: var(--sp-3);
  width: 10px;
  height: 6px;
  color: var(--text-3);
  pointer-events: none;
}
</style>
