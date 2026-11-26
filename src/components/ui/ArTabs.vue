<script setup>
import { nextTick, ref } from 'vue'

const model = defineModel({ type: String, default: '' })
const props = defineProps({
  tabs: { type: Array, default: () => [] },
  size: { type: String, default: 'md' },
  block: Boolean,
})

const tabEls = ref([])

function focusTab(index) {
  nextTick(() => tabEls.value[index]?.focus())
}

function onKeydown(e) {
  const enabled = props.tabs
    .map((tab, index) => ({ tab, index }))
    .filter(({ tab }) => !tab.disabled)
  if (!enabled.length) return

  const current = enabled.findIndex(({ tab }) => tab.key === model.value)
  let next = -1
  if (e.key === 'ArrowRight' || e.key === 'ArrowDown') next = (current + 1) % enabled.length
  else if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') next = (current - 1 + enabled.length) % enabled.length
  else if (e.key === 'Home') next = 0
  else if (e.key === 'End') next = enabled.length - 1
  else return

  e.preventDefault()
  const target = enabled[next]
  model.value = target.tab.key
  focusTab(target.index)
}
</script>

<template>
  <div
    class="ar-tabs"
    :class="[`ar-tabs-${size}`, { 'ar-tabs-block': block }]"
    role="tablist"
    @keydown="onKeydown">
    <button
      v-for="(t, i) in tabs"
      :key="t.key"
      :ref="(el) => (tabEls[i] = el)"
      class="ar-tab"
      :class="{ active: model === t.key }"
      :disabled="t.disabled"
      :tabindex="model === t.key ? 0 : -1"
      :title="t.hint || null"
      role="tab"
      :aria-selected="model === t.key"
      @click="!t.disabled && (model = t.key)">
      {{ t.label }}
    </button>
  </div>
</template>

<style scoped>
.ar-tabs {
  display: inline-flex;
  align-items: center;
  gap: 2px;
  padding: 2px;
  background: var(--bg-field);
  border: 1px solid var(--border);
  border-radius: var(--r-md);
}
.ar-tabs-block { display: flex; width: 100%; }
.ar-tabs-block .ar-tab { flex: 1; }

.ar-tab {
  height: var(--h-control-sm);
  padding: 0 var(--sp-4);
  border: 0;
  border-radius: var(--r-sm);
  background: transparent;
  color: var(--text-2);
  font-family: inherit;
  font-size: var(--fs-sm);
  white-space: nowrap;
  cursor: pointer;
  transition: background var(--dur-fast) var(--ease), color var(--dur-fast) var(--ease);
}
.ar-tabs-md .ar-tab { height: var(--h-control-md); padding: 0 var(--sp-5); }
.ar-tabs-xs .ar-tab { height: var(--h-control-xs); padding: 0 var(--sp-3); font-size: var(--fs-xs); }

.ar-tab:hover:not(:disabled):not(.active) { background: var(--bg-hover); color: var(--text); }
.ar-tab.active {
  background: var(--bg-elev-2);
  color: var(--text);
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.3), inset 0 0 0 1px rgba(255, 255, 255, 0.04);
}
.ar-tab:focus-visible { outline: none; box-shadow: var(--ring); }
.ar-tab:disabled { opacity: 0.38; cursor: default; }
</style>
