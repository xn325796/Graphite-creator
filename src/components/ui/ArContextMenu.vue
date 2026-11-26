<script setup>
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'

const props = defineProps({
  items: { type: Array, default: () => [] },
  x: { type: Number, default: 0 },
  y: { type: Number, default: 0 },
})
const open = defineModel('open', { type: Boolean, default: false })
const emit = defineEmits(['choose', 'close'])

const menuEl = ref(null)
const pos = ref({ left: 0, top: 0 })
const activeIndex = ref(-1)

const selectable = computed(() =>
  props.items.map((it, i) => ({ it, i })).filter(({ it }) => !it.sep && !it.disabled))

function close(focusBack = false) {
  open.value = false
  emit('close')
  if (focusBack) document.activeElement?.blur?.()
}

function choose(item) {
  if (!item || item.sep || item.disabled) return
  open.value = false
  emit('choose', item)
  item.action?.()
}

// 贴边时整体上移/左移，菜单始终完整留在窗口内
async function place() {
  const el = menuEl.value
  if (!el) return
  const margin = 8
  const r = el.getBoundingClientRect()
  let left = props.x
  let top = props.y
  if (left + r.width > window.innerWidth - margin) left = Math.max(margin, window.innerWidth - margin - r.width)
  if (top + r.height > window.innerHeight - margin) top = Math.max(margin, window.innerHeight - margin - r.height)
  pos.value = { left, top }
}

function moveActive(step) {
  const list = selectable.value
  if (!list.length) return
  const cur = list.findIndex(({ i }) => i === activeIndex.value)
  const next = cur < 0
    ? (step > 0 ? 0 : list.length - 1)
    : (cur + step + list.length) % list.length
  activeIndex.value = list[next].i
}

function onKey(e) {
  if (e.key === 'ArrowDown') { e.preventDefault(); moveActive(1) }
  else if (e.key === 'ArrowUp') { e.preventDefault(); moveActive(-1) }
  else if (e.key === 'Home') { e.preventDefault(); activeIndex.value = selectable.value[0]?.i ?? -1 }
  else if (e.key === 'End') { e.preventDefault(); activeIndex.value = selectable.value.at(-1)?.i ?? -1 }
  else if (e.key === 'Enter' || e.key === ' ') { e.preventDefault(); choose(props.items[activeIndex.value]) }
  else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); close(true) }
  else if (e.key === 'Tab') close()
}

function onOutside(e) {
  if (!open.value) return
  if (!menuEl.value?.contains(e.target)) close()
}
const onBlur = () => { if (open.value) close() }
const onResize = () => { if (open.value) close() }

watch(open, async (v) => {
  if (!v) return
  activeIndex.value = -1
  pos.value = { left: props.x, top: props.y }
  await nextTick()
  place()
  menuEl.value?.focus()
})

onMounted(() => {
  document.addEventListener('pointerdown', onOutside, true)
  window.addEventListener('blur', onBlur)
  window.addEventListener('resize', onResize)
})
onUnmounted(() => {
  document.removeEventListener('pointerdown', onOutside, true)
  window.removeEventListener('blur', onBlur)
  window.removeEventListener('resize', onResize)
})
</script>

<template>
  <Teleport to="body">
    <transition name="ar-ctx">
      <div
        v-if="open"
        ref="menuEl"
        class="ar-ctx"
        :style="{ left: pos.left + 'px', top: pos.top + 'px' }"
        role="menu"
        tabindex="-1"
        @keydown="onKey">
        <template v-for="(it, i) in items" :key="i">
          <div v-if="it.sep" class="ar-ctx-sep" role="separator"></div>
          <button
            v-else
            class="ar-ctx-item"
            :class="{ active: i === activeIndex, danger: it.danger, disabled: it.disabled }"
            :disabled="it.disabled"
            role="menuitem"
            @click="choose(it)"
            @pointerenter="activeIndex = i">
            <span class="ar-ctx-label">{{ it.label }}</span>
            <span v-if="it.shortcut" class="ar-ctx-shortcut">{{ it.shortcut }}</span>
          </button>
        </template>
      </div>
    </transition>
  </Teleport>
</template>

<style scoped>
.ar-ctx {
  position: fixed;
  z-index: var(--z-popover);
  min-width: 184px;
  max-height: min(60vh, 420px);
  overflow-y: auto;
  padding: var(--sp-1);
  background: var(--bg-elev);
  border: 1px solid var(--border-strong);
  border-radius: var(--r-md);
  box-shadow: var(--shadow-pop);
  display: flex;
  flex-direction: column;
}
.ar-ctx:focus { outline: none; }

.ar-ctx-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-4);
  min-height: var(--h-row);
  padding: 0 var(--sp-3);
  border-radius: var(--r-xs);
  color: var(--text);
  font-family: inherit;
  font-size: var(--fs-sm);
  text-align: left;
}
.ar-ctx-item.active:not(.disabled) { background: var(--bg-elev-2); }
.ar-ctx-item:disabled { color: var(--text-3); cursor: default; }
.ar-ctx-item.danger:not(.disabled) { color: var(--danger); }
.ar-ctx-item.danger.active:not(.disabled) { background: var(--danger-soft); }
.ar-ctx-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ar-ctx-shortcut { color: var(--text-3); font-size: var(--fs-2xs); }
.ar-ctx-sep { height: 1px; margin: var(--sp-1) var(--sp-2); background: var(--border); }

.ar-ctx-enter-active { transition: opacity var(--dur-fast) var(--ease); }
.ar-ctx-leave-active { transition: opacity var(--dur-fast) ease; }
.ar-ctx-enter-from, .ar-ctx-leave-to { opacity: 0; }
</style>
