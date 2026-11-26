<script setup>
import { computed, nextTick, reactive, ref, watch } from 'vue'
import {
  S, tr, selected, select, setType, setFilter, setView,
  typeRecords, pageRecords, issuesFor, addRecord, deleteSelected, deleteRecord,
  displayName, spriteUrl, prefetchSprites, isVanilla, TYPES,
} from '../../store/project.js'
import { TYPE_LABEL, TYPE_COLOR } from '../../store/schema.js'
import { usePanelWidth } from '../../lib/panelWidth.js'
import { focusInspectorName } from '../../lib/actions.js'
import ArButton from '../ui/ArButton.vue'
import ArInput from '../ui/ArInput.vue'
import ArTabs from '../ui/ArTabs.vue'
import ArBadge from '../ui/ArBadge.vue'
import ArContextMenu from '../ui/ArContextMenu.vue'

const sel = computed(() => selected())
const lang = computed(() => S.ui.lang)
const typeLabel = (tp) => TYPE_LABEL[lang.value][tp]
const typeCount = (tp) => (S.project ? S.project.records.filter((r) => r.type === tp).length : 0)

const {
  width: sidebarWidth,
  startResize: startSidebarResize,
  reset: resetSidebar,
} = usePanelWidth({ key: 'graphite.assetsSidebarWidth', initial: 232, min: 180, max: 380, edge: 'right' })

const recordColor = (r) => {
  const c = r.fields?.color
  return typeof c === 'string' && c ? `#${c.replace(/^#/, '')}` : TYPE_COLOR[r.type]
}
const cardTip = (r) => {
  const n = issuesFor(r).length
  return `${displayName(r)} · ${r.name}${n ? ` · ${tr('list.issues', { n })}` : ''}`
}
const hasError = (r) => issuesFor(r).some((i) => i.level === 'error')

// ---------- 右键菜单（卡片/行） ----------
const ctx = reactive({ open: false, x: 0, y: 0, record: null })
const ctxItems = computed(() => {
  const r = ctx.record
  if (!r) return []
  return [
    { label: tr('ctx.rename'), action: () => focusInspectorName() },
    { sep: true },
    { label: tr('list.delete'), danger: true, action: () => deleteRecord(r) },
  ]
})
function openCtxAt(x, y, r) {
  if (!r) return
  if (S.selectedId !== r.id) select(r)
  ctx.record = r
  ctx.x = x
  ctx.y = y
  ctx.open = true
}
function onCardCtx(e, r) {
  openCtxAt(e.clientX, e.clientY, r)
}
function onCardKey(e, r) {
  if (e.key !== 'ContextMenu' && !(e.shiftKey && e.key === 'F10')) return
  e.preventDefault()
  const rect = e.currentTarget?.getBoundingClientRect?.()
  openCtxAt(rect ? rect.left + 12 : e.clientX, rect ? rect.top + 12 : e.clientY, r)
}

// ---------- 左侧类型 listbox：方向键 ----------
const navEls = ref([])
function onNavKey(e) {
  const list = TYPES
  let i = Math.max(0, list.indexOf(S.ui.type))
  if (e.key === 'ArrowDown') i = (i + 1) % list.length
  else if (e.key === 'ArrowUp') i = (i - 1 + list.length) % list.length
  else if (e.key === 'Home') i = 0
  else if (e.key === 'End') i = list.length - 1
  else return
  e.preventDefault()
  setType(list[i])
  nextTick(() => navEls.value[i]?.focus())
}

// 只预取当前页，避免一次上千请求
watch(
  [pageRecords, () => S.ui.showSprites, () => S.ui.view, () => S.project],
  () => prefetchSprites(pageRecords()),
  { immediate: true },
)
</script>

<template>
  <div class="pane">
    <aside class="pane-sidebar" :style="{ width: sidebarWidth + 'px' }">
      <header class="pane-header">
        <h2 class="pane-heading">{{ tr('list.title') }}</h2>
        <span class="pane-heading-count">{{ typeRecords().length }}</span>
      </header>

      <div class="pane-search">
        <ArInput size="sm" :model-value="S.ui.filter" :placeholder="tr('list.filter')" @input="setFilter($event.target.value)" />
      </div>

      <nav class="pane-nav" role="listbox" :aria-label="tr('list.title')" @keydown="onNavKey">
        <button
          v-for="(tp, ni) in TYPES"
          :key="tp"
          :ref="(el) => (navEls[ni] = el)"
          class="nav-row"
          :class="{ active: S.ui.type === tp }"
          :tabindex="S.ui.type === tp ? 0 : -1"
          role="option"
          :aria-selected="S.ui.type === tp"
          @click="setType(tp)">
          <span class="nav-dot" :style="{ background: TYPE_COLOR[tp] }"></span>
          <span class="nav-label">{{ typeLabel(tp) }}</span>
          <span class="nav-count">{{ typeCount(tp) }}</span>
        </button>
      </nav>

      <footer class="pane-footer">
        <ArButton variant="soft" size="sm" block :disabled="!S.project" @click="addRecord()">
          {{ tr('list.add') }}
        </ArButton>
        <ArButton variant="ghost" size="sm" block :disabled="!sel || sel.type !== S.ui.type" @click="deleteSelected()">
          {{ tr('list.delete') }}
        </ArButton>
      </footer>
    </aside>

    <div class="pane-resizer" title="拖动调整宽度，双击复位" @pointerdown="startSidebarResize" @dblclick="resetSidebar"></div>

    <section class="pane-content">
      <header class="content-head">
        <h3 class="content-title">{{ typeLabel(S.ui.type) }}</h3>
        <span class="content-count">{{ tr('list.count', { n: typeRecords().length }) }}</span>
        <span class="content-spacer"></span>
        <ArTabs
          size="xs"
          :tabs="[{ key: 'grid', label: tr('view.grid') }, { key: 'list', label: tr('view.list') }]"
          :model-value="S.ui.view"
          @update:model-value="setView" />
      </header>

      <div class="asset-grid" v-if="S.ui.view === 'grid'">
        <button
          v-for="r in pageRecords()" :key="r.id"
          class="asset-card" :class="{ selected: S.selectedId === r.id }"
          :title="cardTip(r)"
          @click="select(r)"
          @contextmenu.prevent="onCardCtx($event, r)"
          @keydown="onCardKey($event, r)">
          <span class="asset-errdot" v-if="hasError(r)"></span>
          <span class="asset-thumb">
            <img v-if="spriteUrl(r)" :src="spriteUrl(r)" alt="" loading="lazy" />
            <span v-else class="asset-swatch" :style="{ background: recordColor(r) }"></span>
          </span>
          <span class="asset-name">{{ displayName(r) }}</span>
          <span class="asset-id">{{ r.name }}</span>
          <span class="asset-flag" v-if="isVanilla(r.type, r.name)">{{ tr('list.vanilla') }}</span>
        </button>
        <div class="empty-state" v-if="typeRecords().length === 0">
          <strong>{{ tr('list.empty') }}</strong>
          <span>{{ tr('list.emptyHint') }}</span>
        </div>
      </div>

      <div class="asset-list" v-else>
        <div class="asset-row asset-row-head">
          <span></span>
          <span>{{ tr('list.colName') }}</span>
          <span>{{ tr('list.colId') }}</span>
          <span>{{ tr('list.colState') }}</span>
        </div>
        <button
          v-for="r in pageRecords()" :key="r.id"
          class="asset-row" :class="{ selected: S.selectedId === r.id }"
          @click="select(r)"
          @contextmenu.prevent="onCardCtx($event, r)"
          @keydown="onCardKey($event, r)">
          <span class="asset-row-thumb">
            <img v-if="spriteUrl(r)" :src="spriteUrl(r)" alt="" loading="lazy" />
            <span v-else class="asset-swatch sm" :style="{ background: recordColor(r) }"></span>
          </span>
          <span class="asset-row-name">{{ displayName(r) }}</span>
          <span class="asset-row-id">{{ r.name }}</span>
          <span class="asset-row-state">
            <ArBadge v-if="isVanilla(r.type, r.name)" variant="accent">{{ tr('list.vanilla') }}</ArBadge>
            <ArBadge v-if="r.dirty" variant="warn">{{ tr('list.dirty') }}</ArBadge>
            <ArBadge v-if="issuesFor(r).length" :variant="hasError(r) ? 'err' : 'warn'">{{ tr('list.issues', { n: issuesFor(r).length }) }}</ArBadge>
          </span>
        </button>
        <div class="empty-state" v-if="typeRecords().length === 0">
          <strong>{{ tr('list.empty') }}</strong>
        </div>
      </div>
    </section>

    <ArContextMenu v-model:open="ctx.open" :items="ctxItems" :x="ctx.x" :y="ctx.y" />
  </div>
</template>

<style scoped src="./assets.css"></style>
