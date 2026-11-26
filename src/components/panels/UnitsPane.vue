<script setup>
// 单位构建工作区：只显示 type=unit，本地维护筛选/分页/视图，不污染资源管理页的 S.ui.type。
import { computed, reactive, ref, watch } from 'vue'
import {
  S, tr, selected, select, displayName, issuesFor, spriteUrl, isVanilla,
  recordsOfType, matchesQuery, addRecord, deleteSelected, deleteRecord, prefetchSprites,
} from '../../store/project.js'
import { TYPE_COLOR } from '../../store/schema.js'
import { usePanelWidth } from '../../lib/panelWidth.js'
import { focusInspectorName } from '../../lib/actions.js'
import ArButton from '../ui/ArButton.vue'
import ArInput from '../ui/ArInput.vue'
import ArTabs from '../ui/ArTabs.vue'
import ArBadge from '../ui/ArBadge.vue'
import ArContextMenu from '../ui/ArContextMenu.vue'

const UNIT_TYPE = 'unit'
const PER_PAGE = 28

const q = ref('')
const page = ref(0)
const view = ref('grid')

const {
  width: sidebarWidth,
  startResize: startSidebarResize,
  reset: resetSidebar,
} = usePanelWidth({ key: 'graphite.unitsSidebarWidth', initial: 232, min: 180, max: 380, edge: 'right' })

const sel = computed(() => selected())
const all = computed(() => recordsOfType(UNIT_TYPE))
const filtered = computed(() => matchesQuery(all.value, q.value))
const pageCount = computed(() => Math.max(1, Math.ceil(filtered.value.length / PER_PAGE)))
const paged = computed(() => filtered.value.slice(page.value * PER_PAGE, page.value * PER_PAGE + PER_PAGE))

watch(q, () => { page.value = 0 })
watch(pageCount, (n) => { if (page.value >= n) page.value = Math.max(0, n - 1) })
watch([paged, () => S.ui.showSprites, () => S.project], () => prefetchSprites(paged.value), { immediate: true })

const recordColor = (r) => {
  const c = r?.fields?.color
  return typeof c === 'string' && c ? `#${c.replace(/^#/, '')}` : TYPE_COLOR[r?.type] || '#9CB664'
}
const hasError = (r) => issuesFor(r).some((i) => i.level === 'error')
const cardTip = (r) => {
  const n = issuesFor(r).length
  return `${displayName(r)} · ${r.name}${n ? ` · ${tr('list.issues', { n })}` : ''}`
}
const sprite = (r) => spriteUrl(r)

// ---------- 右键菜单 ----------
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
</script>

<template>
  <div class="pane">
    <aside class="pane-sidebar" :style="{ width: sidebarWidth + 'px' }">
      <header class="pane-header">
        <h2 class="pane-heading">{{ tr('ws.units.title') }}</h2>
        <span class="pane-heading-count">{{ all.length }}</span>
      </header>

      <div class="pane-search">
        <ArInput size="sm" :model-value="q" :placeholder="tr('list.filter')" @input="q = $event.target.value; page = 0" />
      </div>

      <div class="pane-note">{{ tr('status.records', { n: all.length }) }}</div>

      <footer class="pane-footer">
        <ArButton variant="soft" size="sm" block :disabled="!S.project" @click="addRecord(UNIT_TYPE)">
          {{ tr('list.add') }}
        </ArButton>
        <ArButton variant="ghost" size="sm" block :disabled="!sel || sel.type !== UNIT_TYPE" @click="deleteSelected(UNIT_TYPE)">
          {{ tr('list.delete') }}
        </ArButton>
      </footer>
    </aside>

    <div class="pane-resizer" title="拖动调整宽度，双击复位" @pointerdown="startSidebarResize" @dblclick="resetSidebar"></div>

    <section class="pane-content">
      <header class="content-head">
        <h3 class="content-title">{{ tr('ws.units.title') }}</h3>
        <span class="content-count">{{ tr('list.count', { n: filtered.length }) }}</span>
        <span class="content-spacer"></span>
        <ArTabs
          size="xs"
          :tabs="[{ key: 'grid', label: tr('view.grid') }, { key: 'list', label: tr('view.list') }]"
          :model-value="view"
          @update:model-value="view = $event; page = 0" />
      </header>

      <div class="asset-grid" v-if="view === 'grid'">
        <button
          v-for="r in paged" :key="r.id"
          class="asset-card" :class="{ selected: S.selectedId === r.id }"
          :title="cardTip(r)"
          @click="select(r)"
          @contextmenu.prevent="onCardCtx($event, r)"
          @keydown="onCardKey($event, r)">
          <span class="asset-errdot" v-if="hasError(r)"></span>
          <span class="asset-thumb">
            <img v-if="sprite(r)" :src="sprite(r)" alt="" loading="lazy" />
            <span v-else class="asset-swatch" :style="{ background: recordColor(r) }"></span>
          </span>
          <span class="asset-name">{{ displayName(r) }}</span>
          <span class="asset-id">{{ r.name }}</span>
          <span class="asset-flag" v-if="isVanilla(r.type, r.name)">{{ tr('list.vanilla') }}</span>
        </button>
        <div class="empty-state" v-if="filtered.length === 0">
          <strong>{{ tr('list.empty') }}</strong>
          <span>{{ q ? tr('list.filter') : tr('list.emptyHint') }}</span>
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
          v-for="r in paged" :key="r.id"
          class="asset-row" :class="{ selected: S.selectedId === r.id }"
          @click="select(r)"
          @contextmenu.prevent="onCardCtx($event, r)"
          @keydown="onCardKey($event, r)">
          <span class="asset-row-thumb">
            <img v-if="sprite(r)" :src="sprite(r)" alt="" loading="lazy" />
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
        <div class="empty-state" v-if="filtered.length === 0">
          <strong>{{ tr('list.empty') }}</strong>
          <span>{{ q ? tr('list.filter') : tr('list.emptyHint') }}</span>
        </div>
      </div>

      <footer class="content-foot" v-if="pageCount > 1">
        <ArButton variant="ghost" size="xs" icon :disabled="page === 0" @click="page--">‹</ArButton>
        <span class="content-page">{{ page + 1 }} / {{ pageCount }}</span>
        <ArButton variant="ghost" size="xs" icon :disabled="page >= pageCount - 1" @click="page++">›</ArButton>
      </footer>
    </section>

    <ArContextMenu v-model:open="ctx.open" :items="ctxItems" :x="ctx.x" :y="ctx.y" />
  </div>
</template>

<style scoped src="./assets.css"></style>
