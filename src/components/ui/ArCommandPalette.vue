<script setup>
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import { tr } from '../../store/project.js'
import {
  commands, commandById, isCommandEnabled, runCommand, MENU_LAYOUT,
} from '../../lib/actions.js'

const open = defineModel('open', { type: Boolean, default: false })
const emit = defineEmits(['close'])

const query = ref('')
const active = ref(0)
const inputEl = ref(null)
const listEl = ref(null)
const RECENT_KEY = 'graphite.palette.recent'
// 一屏只渲染前 60 条：上百条命令时也不拖慢键盘移动与滚动
const RESULT_LIMIT = 60

const menuLabel = (id) => MENU_LAYOUT.find((m) => m.id === id)?.label() || tr('palette.all')

function readRecent() {
  try {
    const ids = JSON.parse(localStorage.getItem(RECENT_KEY) || '[]')
    return Array.isArray(ids) ? ids.map(commandById).filter(Boolean) : []
  } catch { return [] }
}
const recent = ref(readRecent())

function pushRecent(cmd) {
  const ids = [...recent.value.map((c) => c.id).filter((id) => id !== cmd.id), cmd.id].slice(-8)
  try { localStorage.setItem(RECENT_KEY, JSON.stringify(ids)) } catch { /* noop */ }
  recent.value = ids.map(commandById).filter(Boolean)
}

function score(cmd, q) {
  if (!q) return 0
  const title = cmd.title().toLowerCase()
  if (title.startsWith(q)) return 0
  if (title.includes(q)) return 1
  if (cmd.id.toLowerCase().includes(q)) return 2
  if (menuLabel(cmd.menu).toLowerCase().includes(q)) return 3
  return -1
}

const results = computed(() => {
  const q = query.value.trim().toLowerCase()
  const list = commands
    .map((cmd) => ({ cmd, s: score(cmd, q) }))
    .filter(({ s }) => q === '' || s >= 0)
    // 同分时短标题优先，避免一屏里长名字把精确项挤下去
    .sort((a, b) => a.s - b.s
      || a.cmd.title().length - b.cmd.title().length
      || a.cmd.title().localeCompare(b.cmd.title()))
    .slice(0, RESULT_LIMIT)
    .map(({ cmd }) => cmd)
  return list
})

// 最近使用置顶（仅无查询时）
const groups = computed(() => {
  if (query.value.trim()) {
    return [{ label: tr('palette.all'), items: results.value }]
  }
  const recentIds = new Set(recent.value.map((c) => c.id))
  const out = []
  if (recent.value.length) out.push({ label: tr('palette.recent'), items: recent.value })
  const rest = results.value.filter((c) => !recentIds.has(c.id))
  const byMenu = new Map()
  for (const cmd of rest) {
    if (!byMenu.has(cmd.menu)) byMenu.set(cmd.menu, [])
    byMenu.get(cmd.menu).push(cmd)
  }
  for (const [menu, items] of byMenu) out.push({ label: menuLabel(menu), items })
  return out
})

const flat = computed(() => groups.value.flatMap((g) => g.items))

let restoreFocus = null

watch(open, async (v) => {
  if (v) {
    restoreFocus = document.activeElement
    query.value = ''
    active.value = 0
    recent.value = readRecent()
    await nextTick()
    inputEl.value?.focus()
    return
  }
  const target = restoreFocus
  restoreFocus = null
  if (target instanceof HTMLElement && target.isConnected) {
    nextTick(() => target.focus())
  }
})

function onWindowKey(e) {
  if (!open.value) return
  if (e.key === 'Escape') {
    e.preventDefault()
    e.stopPropagation()
    close()
  } else if (e.key === 'Tab') {
    // 面板是「输入框 + 列表」的单焦点模型，Tab 不允许把焦点漏到被遮挡的页面
    e.preventDefault()
    inputEl.value?.focus()
  }
}

onMounted(() => window.addEventListener('keydown', onWindowKey, true))
onUnmounted(() => window.removeEventListener('keydown', onWindowKey, true))

watch(query, () => { active.value = 0 })

function move(step) {
  const n = flat.value.length
  if (!n) return
  active.value = (active.value + step + n) % n
  nextTick(() => {
    listEl.value?.querySelector('.cp-row.active')?.scrollIntoView({ block: 'nearest' })
  })
}

function close() {
  open.value = false
  emit('close')
}

function run(cmd) {
  if (!cmd) return
  if (!isCommandEnabled(cmd)) return
  pushRecent(cmd)
  close()
  runCommand(cmd)
}

function onKey(e) {
  if (e.key === 'ArrowDown') { e.preventDefault(); move(1) }
  else if (e.key === 'ArrowUp') { e.preventDefault(); move(-1) }
  else if (e.key === 'PageDown') { e.preventDefault(); move(8) }
  else if (e.key === 'PageUp') { e.preventDefault(); move(-8) }
  else if (e.key === 'Enter') { e.preventDefault(); run(flat.value[active.value]) }
}

function rowIndex(cmd) { return flat.value.indexOf(cmd) }
function highlight(text) {
  const q = query.value.trim()
  if (!q) return [{ t: text, hit: false }]
  const i = text.toLowerCase().indexOf(q.toLowerCase())
  if (i < 0) return [{ t: text, hit: false }]
  return [
    { t: text.slice(0, i), hit: false },
    { t: text.slice(i, i + q.length), hit: true },
    { t: text.slice(i + q.length), hit: false },
  ].filter((s) => s.t)
}
</script>

<template>
  <Teleport to="body">
    <transition name="cp">
      <div v-if="open" class="cp-mask" @pointerdown.self="close">
        <div class="cp" role="dialog" aria-modal="true" :aria-label="tr('palette.title')">
          <div class="cp-head">
            <svg class="cp-ico" viewBox="0 0 16 16" aria-hidden="true">
              <circle cx="7" cy="7" r="4.6" fill="none" stroke="currentColor" stroke-width="1.4" />
              <path d="M10.4 10.4L14 14" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
            </svg>
            <input
              ref="inputEl"
              v-model="query"
              class="cp-input"
              :placeholder="tr('palette.placeholder')"
              spellcheck="false"
              @keydown="onKey" />
            <kbd>Esc</kbd>
          </div>

          <div ref="listEl" class="cp-list" role="listbox">
            <p v-if="!flat.length" class="cp-empty">{{ tr('palette.empty') }}</p>
            <template v-for="g in groups" :key="g.label">
              <div class="cp-group" v-if="g.items.length">{{ g.label }}</div>
              <button
                v-for="cmd in g.items"
                :key="g.label + cmd.id"
                class="cp-row"
                :class="{ active: rowIndex(cmd) === active, disabled: !isCommandEnabled(cmd) }"
                role="option"
                :aria-selected="rowIndex(cmd) === active"
                :disabled="!isCommandEnabled(cmd)"
                :title="cmd.hint ? (typeof cmd.hint === 'function' ? cmd.hint() : cmd.hint) : ''"
                @pointerenter="active = rowIndex(cmd)"
                @click="run(cmd)">
                <span class="cp-label">
                  <template v-for="(seg, i) in highlight(cmd.title())" :key="i">
                    <mark v-if="seg.hit">{{ seg.t }}</mark><span v-else>{{ seg.t }}</span>
                  </template>
                </span>
                <span v-if="cmd.shortcut" class="cp-shortcut">{{ cmd.shortcut }}</span>
              </button>
            </template>
          </div>

          <footer class="cp-foot">
            <span>{{ tr('palette.hint') }}</span>
            <span class="cp-count">{{ flat.length }}</span>
          </footer>
        </div>
      </div>
    </transition>
  </Teleport>
</template>

<style scoped>
.cp-mask {
  position: fixed;
  inset: 0;
  z-index: var(--z-modal);
  display: flex;
  justify-content: center;
  align-items: flex-start;
  padding: 12vh var(--sp-6) var(--sp-6);
  background: rgba(4, 7, 9, 0.5);
  backdrop-filter: blur(2px);
}

.cp {
  width: min(640px, 100%);
  max-height: 70vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  background: var(--bg-elev);
  border: 1px solid var(--border-strong);
  border-radius: var(--r-lg);
  box-shadow: var(--shadow-modal);
}

.cp-head {
  flex: none;
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  height: 48px;
  padding: 0 var(--sp-4);
  border-bottom: 1px solid var(--border);
}
.cp-ico { width: 16px; height: 16px; flex: none; color: var(--text-3); }
.cp-input {
  flex: 1;
  min-width: 0;
  height: 100%;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--text);
  font-family: inherit;
  font-size: var(--fs-lg);
}
.cp-input::placeholder { color: var(--text-3); }

.cp-list {
  flex: 1 1 auto;
  min-height: 0;
  overflow-y: auto;
  padding: var(--sp-2);
}

.cp-group {
  padding: var(--sp-3) var(--sp-3) var(--sp-1);
  color: var(--text-3);
  font-size: var(--fs-2xs);
  font-weight: 600;
  letter-spacing: 0.08em;
  text-transform: uppercase;
}

.cp-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: var(--sp-4);
  width: 100%;
  min-height: 34px;
  padding: 0 var(--sp-3);
  border-radius: var(--r-sm);
  color: var(--text);
  font-family: inherit;
  font-size: var(--fs-md);
  text-align: left;
}
.cp-row.active:not(.disabled) { background: var(--bg-elev-2); }
.cp-row.disabled { color: var(--text-3); cursor: default; }
.cp-row mark {
  background: transparent;
  color: var(--accent);
  font-weight: 600;
}
.cp-shortcut {
  flex: none;
  color: var(--text-3);
  font-size: var(--fs-2xs);
  letter-spacing: 0.03em;
}

.cp-empty {
  padding: var(--sp-8);
  color: var(--text-3);
  font-size: var(--fs-sm);
  text-align: center;
}

.cp-foot {
  flex: none;
  display: flex;
  align-items: center;
  justify-content: space-between;
  height: 30px;
  padding: 0 var(--sp-4);
  border-top: 1px solid var(--border);
  color: var(--text-3);
  font-size: var(--fs-2xs);
}
.cp-count { font-variant-numeric: tabular-nums; }

.cp-enter-active { transition: opacity var(--dur) var(--ease); }
.cp-leave-active { transition: opacity var(--dur-fast) ease; }
.cp-enter-from, .cp-leave-to { opacity: 0; }
.cp-enter-active .cp { transition: transform var(--dur) var(--ease); }
.cp-enter-from .cp { transform: translateY(-6px) scale(0.99); }
</style>
