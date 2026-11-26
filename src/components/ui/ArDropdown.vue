<script setup>
import { ref, computed, nextTick, onMounted, onUnmounted, watch } from 'vue'

const props = defineProps({
  items: { type: Array, default: () => [] },
  align: { type: String, default: 'left' },
  minWidth: { type: Number, default: 220 },
})
const emit = defineEmits(['choose'])

const open = defineModel('open', { type: Boolean, default: false })
const root = ref(null)
const menuEl = ref(null)
const triggerEl = ref(null)
const activeIndex = ref(-1)
const up = ref(false)
const maxH = ref(0)

// 菜单不越出窗口：下方不够时向上展开，仍不够则限高滚动（figcraft 主菜单口径）。
async function placeMenu() {
  if (!open.value) return
  up.value = false
  maxH.value = 0
  await nextTick()
  const el = menuEl.value
  const anchor = root.value?.getBoundingClientRect()
  if (!el || !anchor) return
  const gap = 5
  const margin = 8
  const below = window.innerHeight - anchor.bottom - margin - gap
  const above = anchor.top - margin - gap
  const need = el.scrollHeight
  if (need > below && above > below) {
    up.value = true
    maxH.value = Math.max(140, above)
  } else if (need > below) {
    maxH.value = Math.max(140, below)
  }
}

const selectable = computed(() =>
  props.items.map((it, i) => ({ it, i })).filter(({ it }) => !it.sep && !it.header && !it.disabled))

function toggle() {
  open.value = !open.value
}
function close(focusTrigger = false) {
  open.value = false
  if (focusTrigger) nextTick(() => triggerEl.value?.focus())
}
function choose(item) {
  if (!item || item.disabled || item.sep || item.header) return
  close(true)
  emit('choose', item)
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

function onMenuKey(e) {
  if (!open.value) return
  if (e.key === 'ArrowDown') { e.preventDefault(); moveActive(1) }
  else if (e.key === 'ArrowUp') { e.preventDefault(); moveActive(-1) }
  else if (e.key === 'Home') { e.preventDefault(); activeIndex.value = selectable.value[0]?.i ?? -1 }
  else if (e.key === 'End') { e.preventDefault(); activeIndex.value = selectable.value.at(-1)?.i ?? -1 }
  else if (e.key === 'Enter' || e.key === ' ') {
    e.preventDefault()
    choose(props.items[activeIndex.value])
  } else if (e.key === 'Escape') { e.preventDefault(); close(true) }
  else if (e.key === 'Tab') { close() }
}

function onTriggerKey(e) {
  if (e.key === 'ArrowDown' || e.key === 'Enter' || e.key === ' ') {
    e.preventDefault()
    open.value = true
  }
}

function onDocPointer(e) {
  if (!open.value) return
  if (root.value && !root.value.contains(e.target) && !menuEl.value?.contains(e.target)) close()
}
function onWindowBlur() { if (open.value) close() }

watch(open, async (v) => {
  if (v) {
    activeIndex.value = -1
    await nextTick()
    menuEl.value?.focus()
    placeMenu()
  }
})

const onResize = () => { if (open.value) placeMenu() }

onMounted(() => {
  document.addEventListener('pointerdown', onDocPointer, true)
  window.addEventListener('blur', onWindowBlur)
  window.addEventListener('resize', onResize)
})
onUnmounted(() => {
  document.removeEventListener('pointerdown', onDocPointer, true)
  window.removeEventListener('blur', onWindowBlur)
  window.removeEventListener('resize', onResize)
})
</script>

<template>
  <span ref="root" class="ar-dd">
    <span ref="triggerEl" class="ar-dd-trigger" tabindex="-1" @keydown="onTriggerKey">
      <slot name="trigger" :open="open" :toggle="toggle" />
    </span>
    <transition name="ar-pop">
      <div
        v-if="open"
        ref="menuEl"
        class="ar-dd-menu"
        :class="[`ar-dd-${align}`, { 'ar-dd-up': up }]"
        :style="{ minWidth: minWidth + 'px', maxHeight: maxH ? maxH + 'px' : undefined }"
        role="menu"
        tabindex="-1"
        @keydown="onMenuKey">
        <template v-for="(it, i) in items" :key="i">
          <div v-if="it.sep" class="ar-dd-sep" role="separator"></div>
          <div v-else-if="it.header" class="ar-dd-header">{{ it.header }}</div>
          <button
            v-else
            class="ar-dd-item"
            :class="{ checked: it.checked, active: i === activeIndex }"
            :disabled="it.disabled"
            :title="it.hint || null"
            role="menuitem"
            @click.stop="choose(it)"
            @pointerenter="activeIndex = i">
            <span class="ar-dd-check" aria-hidden="true">
              <svg v-if="it.checked" viewBox="0 0 12 12"><path d="M2 6.2l2.6 2.6L10 3.4" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" /></svg>
            </span>
            <span class="ar-dd-label">{{ it.label }}</span>
            <span v-if="it.shortcut" class="ar-dd-shortcut">{{ it.shortcut }}</span>
          </button>
        </template>
      </div>
    </transition>
  </span>
</template>

<style scoped>
.ar-dd { position: relative; display: inline-flex; }
.ar-dd-trigger { display: inline-flex; }

.ar-dd-menu {
  position: absolute;
  top: calc(100% + 5px);
  z-index: var(--z-popover);
  max-height: min(72vh, 560px);
  overflow-y: auto;
  padding: var(--sp-1);
  background: var(--bg-elev);
  border: 1px solid var(--border-strong);
  border-radius: var(--r-md);
  box-shadow: var(--shadow-pop);
  display: flex;
  flex-direction: column;
}
.ar-dd-menu:focus { outline: none; }
.ar-dd-left { left: 0; }
.ar-dd-right { right: 0; }
.ar-dd-up { top: auto; bottom: calc(100% + 5px); }

.ar-dd-item {
  display: grid;
  grid-template-columns: 16px minmax(0, 1fr) auto;
  align-items: center;
  gap: var(--sp-2);
  min-height: var(--h-row);
  padding: 0 var(--sp-3);
  border-radius: var(--r-xs);
  background: transparent;
  color: var(--text);
  font-family: inherit;
  font-size: var(--fs-sm);
  text-align: left;
  cursor: pointer;
}
.ar-dd-item.active:not(:disabled),
.ar-dd-item:hover:not(:disabled) { background: var(--bg-elev-2); }
.ar-dd-item:active:not(:disabled) { background: var(--bg-active); }
.ar-dd-item:disabled { color: var(--text-3); cursor: default; }
.ar-dd-item.checked { color: var(--accent); }

.ar-dd-check { display: inline-flex; width: 16px; height: 16px; color: var(--accent); }
.ar-dd-check svg { width: 12px; height: 12px; }
.ar-dd-label { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.ar-dd-shortcut {
  padding-left: var(--sp-5);
  color: var(--text-3);
  font-size: var(--fs-2xs);
  letter-spacing: 0.03em;
  white-space: nowrap;
}
.ar-dd-header {
  padding: var(--sp-2) var(--sp-3) var(--sp-1);
  color: var(--text-3);
  font-size: var(--fs-2xs);
  font-weight: 600;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}
.ar-dd-sep { height: 1px; margin: var(--sp-1) var(--sp-2); background: var(--border); }

.ar-pop-enter-active { transition: opacity var(--dur-fast) var(--ease), transform var(--dur-fast) var(--ease); }
.ar-pop-leave-active { transition: opacity var(--dur-fast) ease; }
.ar-pop-enter-from { opacity: 0; transform: translateY(-4px); }
.ar-pop-leave-to { opacity: 0; }
</style>
