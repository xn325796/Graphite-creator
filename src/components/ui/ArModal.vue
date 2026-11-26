<script setup>
import { ref, watch, nextTick, onMounted, onUnmounted } from 'vue'
import { modalState } from '../../lib/modalState.js'

const open = defineModel('open', { type: Boolean, default: false })
const props = defineProps({
  title: String,
  width: { type: Number, default: 0 },
  size: { type: String, default: 'md' }, // sm 400 · md 520 · lg 760
  dismissible: { type: Boolean, default: true },
  confirmOnEnter: { type: Boolean, default: true },
})
const emit = defineEmits(['close'])

const modalEl = ref(null)
let counted = false
let restoreFocus = null

const SIZES = { sm: 400, md: 520, lg: 760 }
const dialogWidth = () => props.width || SIZES[props.size] || SIZES.md

function close() {
  if (!props.dismissible) return
  open.value = false
  emit('close')
}

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]), textarea:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])'
const focusables = () =>
  [...(modalEl.value?.querySelectorAll(FOCUSABLE) || [])].filter((el) => el.getClientRects().length)

function onKey(e) {
  if (!open.value) return
  if (e.key === 'Escape') { e.stopPropagation(); close(); return }
  if (e.key === 'Enter' && props.confirmOnEnter) {
    const t = e.target
    if (t instanceof HTMLElement && (t.tagName === 'TEXTAREA' || t.closest('textarea'))) return
    // 焦点已在页脚按钮上时交给按钮自己的 click，避免重复触发
    if (t instanceof HTMLElement && t.closest('.ar-modal-foot')) return
    const btn = modalEl.value?.querySelector('.ar-modal-foot .ar-btn-primary:not([disabled]), .ar-modal-foot .ar-btn-danger:not([disabled])')
    if (btn) { e.preventDefault(); btn.click() }
    return
  }
  if (e.key !== 'Tab') return
  const items = focusables()
  if (!items.length) { e.preventDefault(); modalEl.value?.focus(); return }
  const first = items[0]
  const last = items[items.length - 1]
  const active = document.activeElement
  const inside = modalEl.value?.contains(active)
  if (e.shiftKey && (!inside || active === first)) { e.preventDefault(); last.focus() }
  else if (!e.shiftKey && (!inside || active === last)) { e.preventDefault(); first.focus() }
}

watch(open, async (v, old) => {
  if (v) {
    if (!counted) { modalState.open++; counted = true }
    restoreFocus = document.activeElement
    await nextTick()
    // 聚焦对话框容器而非首个控件：避免关闭按钮一打开就出现键盘焦点环
    modalEl.value?.focus()
  } else {
    if (counted) { modalState.open--; counted = false }
    if (restoreFocus instanceof HTMLElement) restoreFocus.focus()
    restoreFocus = null
    if (old) emit('close')
  }
}, { immediate: true })

onMounted(() => window.addEventListener('keydown', onKey, true))
onUnmounted(() => {
  window.removeEventListener('keydown', onKey, true)
  if (counted) { modalState.open--; counted = false }
})
</script>

<template>
  <Teleport to="body">
    <transition name="ar-modal">
      <div v-if="open" class="ar-modal-mask" @mousedown.self="close">
        <div
          ref="modalEl"
          class="ar-modal"
          :style="{ width: dialogWidth() + 'px' }"
          role="dialog"
          aria-modal="true"
          tabindex="-1">
          <header class="ar-modal-head">
            <h2 class="ar-modal-title">{{ title }}</h2>
            <button v-if="dismissible" class="ar-modal-close" aria-label="Close" @click="close">
              <svg viewBox="0 0 12 12" aria-hidden="true">
                <path d="M1 1l10 10M11 1L1 11" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
              </svg>
            </button>
          </header>
          <div class="ar-modal-body"><slot /></div>
          <footer v-if="$slots.footer" class="ar-modal-foot"><slot name="footer" /></footer>
        </div>
      </div>
    </transition>
  </Teleport>
</template>

<style scoped>
.ar-modal-mask {
  position: fixed;
  inset: 0;
  z-index: var(--z-modal);
  display: flex;
  align-items: center;
  justify-content: center;
  padding: var(--sp-6);
  background: rgba(4, 7, 9, 0.62);
  backdrop-filter: blur(2px);
}

.ar-modal {
  max-width: 100%;
  max-height: calc(100vh - 48px);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--bg-panel);
  border: 1px solid var(--border-strong);
  border-radius: var(--r-xl);
  box-shadow: var(--shadow-modal);
}
.ar-modal:focus { outline: none; }

.ar-modal-head {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-4);
  height: 50px;
  padding: 0 var(--sp-5) 0 var(--sp-6);
  border-bottom: 1px solid var(--border-subtle);
}
.ar-modal-title {
  color: var(--text);
  font-size: var(--fs-lg);
  font-weight: 600;
  line-height: var(--lh-tight);
}
.ar-modal-close {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  border-radius: var(--r-sm);
  color: var(--text-3);
  transition: background var(--dur-fast) var(--ease), color var(--dur-fast) var(--ease);
}
.ar-modal-close svg { width: 12px; height: 12px; }
.ar-modal-close:hover { background: var(--bg-hover); color: var(--text); }

.ar-modal-body {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: var(--sp-5) var(--sp-6);
  color: var(--text-2);
  font-size: var(--fs-md);
  line-height: var(--lh);
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
}

.ar-modal-foot {
  flex: none;
  display: flex;
  justify-content: flex-end;
  align-items: center;
  gap: var(--sp-3);
  padding: var(--sp-4) var(--sp-6);
  border-top: 1px solid var(--border-subtle);
  background: rgba(255, 255, 255, 0.015);
}

.ar-modal-enter-active { transition: opacity var(--dur-slow) var(--ease); }
.ar-modal-leave-active { transition: opacity var(--dur) ease; }
.ar-modal-enter-from, .ar-modal-leave-to { opacity: 0; }
.ar-modal-enter-active .ar-modal { transition: transform var(--dur-slow) var(--ease); }
.ar-modal-enter-from .ar-modal { transform: translateY(8px) scale(0.985); }
</style>
