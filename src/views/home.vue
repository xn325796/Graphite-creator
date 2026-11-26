<script setup>
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import {
  S, tr, errorCount, warnCount, openDemo, initEvents,
  save, importCancel,
  selectDefaultCosmos, refreshCosmosFields, refreshTechFields, refreshBoard,
  closeConfirmSave, closeConfirmDiscard, closeConfirmCancel,
} from '../store/project.js'
import {
  paletteState, shortcutsOpen, openPalette, openShortcuts, closeShortcuts, commandById,
  doSave, doUndo, doRedo, doValidate, focusSearch, startBuild,
  shortcutGroups as shortcutGroupsRaw, GESTURES,
} from '../lib/actions.js'
import { modalState } from '../lib/modalState.js'
import { toast } from '../lib/toast.js'
import { usePanelWidth } from '../lib/panelWidth.js'
import ArButton from '../components/ui/ArButton.vue'
import ArInput from '../components/ui/ArInput.vue'
import ArModal from '../components/ui/ArModal.vue'
import ArProgress from '../components/ui/ArProgress.vue'
import ArBadge from '../components/ui/ArBadge.vue'
import ArToasts from '../components/ui/ArToasts.vue'
import ArCommandPalette from '../components/ui/ArCommandPalette.vue'
import CosmosPane from '../components/panels/CosmosPane.vue'
import TechPane from '../components/panels/TechPane.vue'
import CanvasPane from '../components/panels/CanvasPane.vue'
import InspectorPane from '../components/panels/InspectorPane.vue'
import AssetsListPane from '../components/panels/AssetsListPane.vue'
import UnitsPane from '../components/panels/UnitsPane.vue'
import MenuBar from '../components/layout/MenuBar.vue'
import { clearDrafts } from '../shared/fieldDrafts.js'

// ---------- 工作区页签 ----------

const wsTabs = computed(() => [
  { key: 'assets', label: tr('ws.assets') },
  { key: 'canvas', label: tr('ws.canvas') },
  { key: 'units', label: tr('ws.units') },
  { key: 'cosmos', label: tr('ws.cosmos') },
  { key: 'tech', label: tr('ws.tech') },
  { key: 'script', label: tr('ws.script'), disabled: true, hint: tr('ws.scriptDisabled') },
])

function setWorkspace(w) {
  const tab = wsTabs.value.find((t) => t.key === w)
  if (tab?.disabled) return
  S.ui.workspace = w
  if (w === 'cosmos') { selectDefaultCosmos(); refreshCosmosFields() }
  if (w === 'tech') refreshTechFields()
  if (w === 'canvas') refreshBoard()
}

const tabEls = ref([])
function tabKeydown(e, i) {
  if (e.key !== 'ArrowRight' && e.key !== 'ArrowLeft') return
  e.preventDefault()
  const step = e.key === 'ArrowRight' ? 1 : -1
  const list = wsTabs.value
  let next = i
  for (let n = 0; n < list.length; n++) {
    next = (next + step + list.length) % list.length
    if (!list[next].disabled) break
  }
  setWorkspace(list[next].key)
  nextTick(() => tabEls.value[next]?.focus())
}

// ---------- 面板宽度 ----------

const isListWorkspace = computed(() => S.ui.workspace === 'assets' || S.ui.workspace === 'units')
const {
  width: inspectorWidth,
  startResize: startInspectorResize,
  reset: resetInspector,
} = usePanelWidth({ key: 'graphite.inspectorWidth', initial: 328, min: 280, max: 560, edge: 'left' })

// ---------- 字段草稿 ----------

watch(() => S.selectedId, clearDrafts)

// ---------- 导入 ----------

const importStageLabel = computed(() => {
  const im = S.ui.importing
  if (!im) return ''
  return tr('dlg.importStage', { stage: tr('stage.' + im.stage), done: im.done, total: im.total })
})

// ---------- 全局对话框 ----------

const modalTitle = computed(() => {
  const kind = S.ui.modal?.kind
  if (kind === 'about') return tr('help.about')
  if (kind === 'meta') return tr('project.meta')
  if (kind === 'closeConfirm') return tr('dlg.closeConfirmTitle')
  return tr('dlg.buildDone')
})

function openIssues() {
  if (!S.project) return
  if (!isListWorkspace.value) S.ui.workspace = 'assets'
  S.ui.inspectorTab = 'validate'
}

// ---------- 快捷键帮助 ----------

const keysQuery = ref('')
const keysFilterRef = ref(null)
watch(() => shortcutsOpen.open, (v) => {
  if (!v) return
  keysQuery.value = ''
  nextTick(() => keysFilterRef.value?.focus())
})
const shortcutGroups = computed(() => {
  const q = keysQuery.value.trim().toLowerCase()
  return shortcutGroupsRaw()
    .map((g) => ({
      ...g,
      items: q
        ? g.items.filter((it) => it.label.toLowerCase().includes(q) || it.keys.toLowerCase().includes(q))
        : g.items,
    }))
    .filter((g) => g.items.length)
})

const gestures = computed(() => {
  const q = keysQuery.value.trim().toLowerCase()
  return GESTURES.map((g) => ({ keys: g.keys, label: g.label() }))
    .filter((g) => !q || g.label.toLowerCase().includes(q) || g.keys.toLowerCase().includes(q))
})

// ---------- 日志 ----------

const logBody = ref(null)
watch(() => S.logs.length, () => {
  nextTick(() => { if (logBody.value) logBody.value.scrollTop = logBody.value.scrollHeight })
})
function clearLog() {
  S.logs.splice(0)
  toast(tr('log.cleared'))
}
async function copyLog() {
  const text = S.logs.map((l) => `${l.time}  ${l.msg}`).join('\n')
  try {
    await navigator.clipboard.writeText(text)
    toast(tr('log.copied'))
  } catch {
    toast(tr('log.copied'))
  }
}

// ---------- 全局键盘 ----------

const inField = (t) =>
  t instanceof Element && !!t.closest('input, textarea, select, [contenteditable="true"]')

function onGlobalKey(e) {
  if (paletteState.open) return
  if (e.key === 'F1') {
    e.preventDefault()
    if (shortcutsOpen.open) closeShortcuts()
    else openShortcuts()
    return
  }
  // 快捷键面板是右侧停靠层：Esc 收起，输入框内的 Esc 由 ArInput 先回退
  if (shortcutsOpen.open && e.key === 'Escape') { e.preventDefault(); closeShortcuts(); return }
  if (modalState.open > 0) return

  if (e.key === 'F10') {
    e.preventDefault()
    document.querySelector('.menubar-item')?.focus()
    return
  }
  if (e.key === 'F5') { e.preventDefault(); doValidate(); return }
  if (e.key === 'F6') { e.preventDefault(); commandById('run.launch').run(); return }

  const mod = e.ctrlKey || e.metaKey
  if (!mod || e.altKey) return
  const k = e.key.toLowerCase()

  if (!e.shiftKey && k === 'k') { e.preventDefault(); openPalette(); return }
  if (e.shiftKey && k === 'p') { e.preventDefault(); openPalette(); return }

  if (inField(e.target)) return

  if (!e.shiftKey) {
    if (k === 's') { e.preventDefault(); doSave(); return }
    if (k === 'z') { e.preventDefault(); doUndo(); return }
    if (k === 'y') { e.preventDefault(); doRedo(); return }
    if (k === 'f') { e.preventDefault(); focusSearch(); return }
    if (k === 'b') { e.preventDefault(); startBuild(); return }
    if (k === '\\') { e.preventDefault(); S.ui.minimal = !S.ui.minimal; return }
    if (k === 'n') { e.preventDefault(); commandById('project.new').run(); return }
    if (k === 'o') { e.preventDefault(); commandById('project.open').run(); return }
    const wsMap = { 1: 'assets', 2: 'canvas', 3: 'units', 4: 'cosmos', 5: 'tech' }
    if (wsMap[e.key]) { e.preventDefault(); setWorkspace(wsMap[e.key]); return }
  }
}

// ---------- 生命周期 ----------

let autosaveTimer = null
onMounted(() => {
  initEvents()
  openDemo()
  window.addEventListener('keydown', onGlobalKey)
  autosaveTimer = setInterval(() => {
    if (S.dirty && S.project) save('auto')
  }, 120000)
})
onUnmounted(() => {
  clearInterval(autosaveTimer)
  window.removeEventListener('keydown', onGlobalKey)
})
</script>

<template>
  <div class="app-shell" :class="{ 'ui-minimal': S.ui.minimal }">
    <MenuBar />

    <div class="workspace-tabs" role="tablist" :aria-label="tr('ws.assets')">
      <button
        v-for="(t, i) in wsTabs"
        :key="t.key"
        :ref="(el) => (tabEls[i] = el)"
        class="ws-tab"
        :class="{ active: S.ui.workspace === t.key, disabled: t.disabled }"
        role="tab"
        :disabled="t.disabled"
        :aria-selected="S.ui.workspace === t.key"
        :title="t.hint || null"
        @click="setWorkspace(t.key)"
        @keydown="tabKeydown($event, i)">
        {{ t.label }}
      </button>
    </div>

    <main class="workspace-area">
      <div v-if="isListWorkspace" class="workspace-split">
        <AssetsListPane v-if="S.ui.workspace === 'assets'" />
        <UnitsPane v-else />
        <div class="inspector-wrap" :style="{ width: inspectorWidth + 'px' }">
          <div class="pane-resizer" title="拖动调整宽度，双击复位" @pointerdown="startInspectorResize" @dblclick="resetInspector"></div>
          <InspectorPane />
        </div>
      </div>
      <CosmosPane v-else-if="S.ui.workspace === 'cosmos'" />
      <TechPane v-else-if="S.ui.workspace === 'tech'" />
      <CanvasPane v-else-if="S.ui.workspace === 'canvas'" />
      <div class="empty-state" v-else>
        <strong>{{ tr('ws.script') }}</strong>
        <span>{{ tr('ws.placeholder') }}</span>
      </div>
    </main>

    <section class="log-drawer" :class="{ open: S.ui.logOpen }" v-show="S.ui.logOpen" aria-label="Log">
      <header class="log-head">
        <span class="log-title">{{ tr('log.title') }}</span>
        <span class="log-count">{{ S.logs.length }}</span>
        <span class="log-spacer"></span>
        <ArButton variant="ghost" size="xs" @click="copyLog">{{ tr('log.copy') }}</ArButton>
        <ArButton variant="ghost" size="xs" :disabled="!S.logs.length" @click="clearLog">{{ tr('log.clear') }}</ArButton>
        <button class="log-close" :aria-label="tr('status.logClose')" @click="S.ui.logOpen = false">
          <svg viewBox="0 0 12 12"><path d="M2.5 2.5l7 7M9.5 2.5l-7 7" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" /></svg>
        </button>
      </header>
      <div ref="logBody" class="log-body selectable">
        <p v-if="!S.logs.length" class="log-empty">{{ tr('log.cleared') }}</p>
        <div class="log-line" v-for="(l, i) in S.logs" :key="i">
          <span class="log-time">{{ l.time }}</span><span class="log-msg">{{ l.msg }}</span>
        </div>
      </div>
    </section>

    <footer class="statusbar">
      <div class="statusbar-left">
        <template v-if="S.project">
          <span class="status-dot" :class="S.dirty ? 'dot-dirty' : 'dot-saved'" :title="tr(S.dirty ? 'status.dirty' : 'status.saved')"></span>
          <span class="status-name">{{ S.project.displayName || S.project.name }}</span>
          <span class="status-meta">· {{ tr('status.records', { n: S.project.records.length }) }}</span>
          <span class="status-path" :title="S.project.path">· {{ S.project.path }}</span>
        </template>
        <span v-else class="status-meta">{{ tr('status.noProject') }}</span>
        <div class="status-import" v-if="S.ui.importing">
          <ArProgress small :value="S.ui.importing.total ? S.ui.importing.done / S.ui.importing.total : 0" />
          <span class="status-import-label">{{ importStageLabel }}</span>
          <ArButton variant="ghost" size="xs" @click="importCancel">{{ tr('dlg.cancel') }}</ArButton>
        </div>
      </div>
      <div class="statusbar-right">
        <button class="status-issue" @click="openIssues" :title="tr('tab.validate')">
          <ArBadge :variant="errorCount() ? 'err' : 'neutral'">{{ errorCount() }}E</ArBadge>
          <ArBadge :variant="warnCount() ? 'warn' : 'neutral'">{{ warnCount() }}W</ArBadge>
        </button>
        <span v-if="S.project" class="status-save">{{ tr(S.dirty ? 'status.dirty' : 'status.saved') }}</span>
        <span v-if="S.savedAt && !S.dirty" class="status-time">{{ tr(S.savedKind === 'auto' ? 'status.autoAt' : 'status.savedAt', { t: S.savedAt }) }}</span>
        <ArButton variant="ghost" size="xs" @click="S.ui.logOpen = !S.ui.logOpen">
          {{ tr(S.ui.logOpen ? 'status.logClose' : 'status.log') }}
        </ArButton>
      </div>
    </footer>

    <ArCommandPalette v-model:open="paletteState.open" />

    <transition name="keys-dock">
      <aside
        v-if="shortcutsOpen.open"
        class="keys-dock"
        role="dialog"
        :aria-label="tr('keys.title')">
        <header class="keys-head">
          <h2 class="keys-title">{{ tr('keys.title') }}</h2>
          <span class="keys-spacer"></span>
          <button class="keys-close" :aria-label="tr('dlg.close')" @click="closeShortcuts">
            <svg viewBox="0 0 12 12"><path d="M2.5 2.5l7 7M9.5 2.5l-7 7" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" /></svg>
          </button>
        </header>
        <div class="keys-tools">
          <ArInput ref="keysFilterRef" size="sm" v-model="keysQuery" :placeholder="tr('keys.filter')" />
          <p class="keys-hint">{{ tr('keys.hint') }}</p>
        </div>
        <div class="keys-scroll">
          <div class="keys-grid">
            <section class="keys-group" v-for="g in shortcutGroups" :key="g.label">
              <h3 class="keys-group-title">{{ g.label }}</h3>
              <div class="keys-row" v-for="it in g.items" :key="g.label + it.label">
                <span class="keys-label">{{ it.label }}</span>
                <kbd>{{ it.keys }}</kbd>
              </div>
            </section>
            <section class="keys-group" v-if="gestures.length">
              <h3 class="keys-group-title">{{ tr('keys.gestures') }}</h3>
              <div class="keys-row" v-for="g in gestures" :key="g.keys">
                <span class="keys-label">{{ g.label }}</span>
                <kbd>{{ g.keys }}</kbd>
              </div>
            </section>
          </div>
        </div>
      </aside>
    </transition>

    <ArModal
      :open="!!S.ui.modal"
      :title="modalTitle"
      :dismissible="S.ui.modal?.kind !== 'closeConfirm' || !S.ui.modal?.busy"
      @close="S.ui.modal = null">
      <template v-if="S.ui.modal?.kind === 'about'">
        <p class="about-name">GraphiteCreator</p>
        <p class="about-line">{{ tr('dlg.aboutBody') }}</p>
      </template>
      <template v-else-if="S.ui.modal?.kind === 'buildDone'"><p>{{ tr('dlg.buildDoneBody', { dir: S.ui.modal.dir }) }}</p></template>
      <template v-else-if="S.ui.modal?.kind === 'closeConfirm'"><p>{{ tr('dlg.closeConfirmBody') }}</p></template>
      <template v-else-if="S.ui.modal?.kind === 'meta' && S.project">
        <div class="fld-ar-row"><div class="fld-ar-label">name</div><ArInput size="sm" :model-value="S.project.meta.name" disabled /></div>
        <div class="fld-ar-row"><div class="fld-ar-label">displayName</div><ArInput size="sm" :model-value="S.project.meta.displayName" disabled /></div>
        <div class="fld-ar-row"><div class="fld-ar-label">minGameVersion</div><ArInput size="sm" :model-value="S.project.meta.minGameVersion" disabled /></div>
      </template>
      <template #footer>
        <template v-if="S.ui.modal?.kind === 'closeConfirm'">
          <ArButton variant="ghost" size="sm" @click="closeConfirmCancel">{{ tr('dlg.cancel') }}</ArButton>
          <ArButton variant="soft" size="sm" :disabled="S.ui.modal?.busy" @click="closeConfirmDiscard">{{ tr('dlg.discardAndClose') }}</ArButton>
          <ArButton variant="primary" size="sm" :disabled="S.ui.modal?.busy" @click="closeConfirmSave">{{ tr('dlg.saveAndClose') }}</ArButton>
        </template>
        <template v-else>
          <ArButton variant="soft" size="sm" @click="S.ui.modal = null">{{ tr('dlg.close') }}</ArButton>
        </template>
      </template>
    </ArModal>

    <ArToasts />
  </div>
</template>

<style scoped src="./home.css"></style>
