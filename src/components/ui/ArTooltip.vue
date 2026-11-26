<script setup>
import { ref, onUnmounted } from 'vue'

const props = defineProps({
  text: { type: String, required: true },
  placement: { type: String, default: 'bottom' },
  /** center | right | left；贴边控件用 right/left 防止气泡撑出窗口 */
  align: { type: String, default: 'center' },
  delay: { type: Number, default: 420 },
})

const shown = ref(false)
let timer = null
function show() {
  clearTimeout(timer)
  timer = setTimeout(() => { shown.value = true }, props.delay)
}
function hide() {
  clearTimeout(timer)
  shown.value = false
}
onUnmounted(() => clearTimeout(timer))
</script>

<template>
  <span
    class="ar-tip-wrap"
    :aria-label="text"
    @mouseenter="show"
    @mouseleave="hide"
    @focusin="show"
    @focusout="hide"
  >
    <slot />
    <span
      v-if="shown"
      class="ar-tip"
      :class="[`ar-tip-${placement}`, `ar-tip-align-${align}`]"
      role="tooltip"
    >{{ text }}</span>
  </span>
</template>

<style scoped>
.ar-tip-wrap { position: relative; display: inline-flex; }
.ar-tip {
  position: absolute;
  left: 50%;
  transform: translate(-50%, 0);
  z-index: var(--z-tooltip);
  padding: 5px 9px;
  background: #05080a;
  border: 1px solid var(--border-strong);
  border-radius: var(--r-sm);
  box-shadow: var(--shadow-pop);
  color: var(--text);
  font-size: var(--fs-xs);
  line-height: 1.3;
  white-space: nowrap;
  pointer-events: none;
  animation: ar-tip-in var(--dur) var(--ease);
}
.ar-tip-top { bottom: calc(100% + 6px); }
.ar-tip-bottom { top: calc(100% + 6px); }
.ar-tip-align-right { left: auto; right: 0; transform: none; }
.ar-tip-align-left { left: 0; transform: none; }
@keyframes ar-tip-in {
  from { opacity: 0; }
  to { opacity: 1; }
}
</style>
