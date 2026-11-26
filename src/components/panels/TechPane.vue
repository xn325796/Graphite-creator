<script setup>
import { ref, reactive, computed, watch } from 'vue'
import {
  S, tr, select, setField, displayName, spriteUrl,
  techForest, readResearch, setResearch, techCandidates,
  OBJECTIVE_TYPES, objectiveTargetKey, objectiveTarget,
} from '../../store/project.js'
import { TYPE_COLOR } from '../../store/schema.js'
import ArButton from '../ui/ArButton.vue'
import ArInput from '../ui/ArInput.vue'
import ArSelect from '../ui/ArSelect.vue'
import ArSwitch from '../ui/ArSwitch.vue'
import ArTabs from '../ui/ArTabs.vue'
import ArBadge from '../ui/ArBadge.vue'

const TECH_NODE = 60
const TECH_GAP = 20
const techRoot = ref(null)
const techSearch = ref('')
const techZoom = ref(1)
const techPan = reactive({ x: 0, y: 0 })
const pickedId = ref(null)
const rubber = ref(null)
const canvasRef = ref(null)

const pickedRecord = computed(() => S.project?.records.find((r) => r.id === pickedId.value) || null)
const forest = computed(() => techForest())
const filteredRoots = computed(() => {
  const q = techSearch.value.trim().toLowerCase()
  const roots = forest.value.roots
  if (!q) return roots
  return roots.filter((r) => r.label.toLowerCase().includes(q) || r.children.some((c) => c.label.toLowerCase().includes(q)))
})
const filteredUnassigned = computed(() => {
  const q = techSearch.value.trim().toLowerCase()
  return forest.value.unassigned.filter((r) => !q || displayName(r).toLowerCase().includes(q) || r.name.toLowerCase().includes(q))
})
const shownRoots = computed(() => {
  const list = filteredRoots.value
  if (techRoot.value == null) return list
  return list[techRoot.value] ? [list[techRoot.value]] : []
})

function makeBox(entry, dir) {
  return { entry, dir, x: 0, y: 0, w: TECH_NODE, children: entry.children.map((c) => makeBox(c, dir)) }
}
function measureBox(b) {
  if (!b.children.length) return (b.w = TECH_NODE)
  let w = 0
  for (const c of b.children) w += measureBox(c) + TECH_GAP
  return (b.w = w - TECH_GAP)
}
function placeBox(b, left) {
  if (!b.children.length) { b.x = left; return }
  let cur = left
  for (const c of b.children) { placeBox(c, cur); cur += c.w + TECH_GAP }
  b.x = (b.children[0].x + b.children[b.children.length - 1].x) / 2
}

const techWorld = computed(() => {
  const nodes = []
  const edges = []
  let cursor = 40
  let minX = 0, minY = 0, maxX = 0, maxY = 0
  for (const rootEntry of shownRoots.value) {
    const half = Math.ceil(rootEntry.children.length / 2)
    const rootBox = { entry: rootEntry, dir: 0, x: 0, y: 0, w: TECH_NODE, children: rootEntry.children.map((c, i) => makeBox(c, i < half ? 1 : -1)) }
    measureBox(rootBox)
    placeBox(rootBox, cursor)
    if (rootBox.children.length) rootBox.x = (rootBox.children[0].x + rootBox.children[rootBox.children.length - 1].x) / 2
    rootBox.y = 0
    const assignY = (b) => {
      for (const c of b.children) { c.y = b.y + c.dir * (TECH_NODE + TECH_GAP); assignY(c) }
    }
    assignY(rootBox)
    cursor += rootBox.w + 120
    const walk = (b) => {
      nodes.push(b)
      for (const c of b.children) {
        const pcy = b.dir >= 0 ? b.y + TECH_NODE : b.y
        const ccy = c.dir >= 0 ? c.y : c.y + TECH_NODE
        edges.push({ x1: b.x + TECH_NODE / 2, y1: pcy, x2: c.x + TECH_NODE / 2, y2: ccy })
        walk(c)
      }
    }
    walk(rootBox)
  }
  for (const n of nodes) {
    minX = Math.min(minX, n.x); minY = Math.min(minY, n.y)
    maxX = Math.max(maxX, n.x + TECH_NODE); maxY = Math.max(maxY, n.y + TECH_NODE)
  }
  return { nodes, edges, minX: minX - 120, minY: minY - 120, maxX: maxX + 120, maxY: maxY + 120 }
})
const worldStyle = computed(() => ({ transform: 'translate(' + techPan.x + 'px,' + techPan.y + 'px) scale(' + techZoom.value + ')' }))

function techWorldPoint(ev) {
  const rect = canvasRef.value?.getBoundingClientRect()
  if (!rect) return { x: 0, y: 0 }
  return { x: (ev.clientX - rect.left - techPan.x) / techZoom.value, y: (ev.clientY - rect.top - techPan.y) / techZoom.value }
}

let panState = null
function onTechPanStart(ev) {
  if (ev.button !== 0 && ev.button !== 1) return
  panState = { sx: ev.clientX, sy: ev.clientY, px: techPan.x, py: techPan.y }
  window.addEventListener('pointermove', onTechPanMove)
  window.addEventListener('pointerup', onTechPanEnd)
}
function onTechPanMove(ev) {
  if (!panState) return
  techPan.x = panState.px + (ev.clientX - panState.sx)
  techPan.y = panState.py + (ev.clientY - panState.sy)
}
function onTechPanEnd() {
  panState = null
  window.removeEventListener('pointermove', onTechPanMove)
  window.removeEventListener('pointerup', onTechPanEnd)
}

function onTechWheel(ev) {
  const rect = canvasRef.value?.getBoundingClientRect()
  if (!rect) return
  const mx = ev.clientX - rect.left
  const my = ev.clientY - rect.top
  const factor = ev.deltaY < 0 ? 1.12 : 1 / 1.12
  const next = Math.min(2.5, Math.max(0.15, techZoom.value * factor))
  if (next === techZoom.value) return
  techPan.x = mx - ((mx - techPan.x) * next) / techZoom.value
  techPan.y = my - ((my - techPan.y) * next) / techZoom.value
  techZoom.value = next
}
function zoomBy(f) {
  techZoom.value = Math.min(2.5, Math.max(0.15, techZoom.value * f))
}
watch(() => S.ui.fitTick, () => fitTech())

function fitTech() {
  const w = techWorld.value
  const rect = canvasRef.value?.getBoundingClientRect()
  if (!rect || w.maxX <= w.minX) return
  const scale = Math.min((rect.width - 40) / (w.maxX - w.minX), (rect.height - 40) / (w.maxY - w.minY), 1.2)
  techZoom.value = Math.max(0.15, scale)
  techPan.x = (rect.width - (w.maxX - w.minX) * techZoom.value) / 2 - w.minX * techZoom.value
  techPan.y = (rect.height - (w.maxY - w.minY) * techZoom.value) / 2 - w.minY * techZoom.value
}

function pickNode(n) {
  pickedId.value = n.entry.recordId
  if (n.entry.record) select(n.entry.record)
}
function portDown(n, ev) {
  if (!n.entry.record) return
  const p = techWorldPoint(ev)
  rubber.value = { sx: n.x + TECH_NODE, sy: n.y + TECH_NODE / 2, ex: p.x, ey: p.y, source: n.entry }
  window.addEventListener('pointermove', onRubberMove)
  window.addEventListener('pointerup', onRubberUp)
}
function onRubberMove(ev) {
  if (!rubber.value) return
  const p = techWorldPoint(ev)
  rubber.value.ex = p.x
  rubber.value.ey = p.y
}
function onRubberUp(ev) {
  window.removeEventListener('pointermove', onRubberMove)
  window.removeEventListener('pointerup', onRubberUp)
  const r = rubber.value
  rubber.value = null
  if (!r) return
  const p = techWorldPoint(ev)
  const target = techWorld.value.nodes.find((n) => p.x >= n.x && p.x <= n.x + TECH_NODE && p.y >= n.y && p.y <= n.y + TECH_NODE)
  if (!target || !target.entry.key || target.entry.recordId === r.source.recordId) return
  const rec = S.project?.records.find((x) => x.id === r.source.recordId)
  if (!rec) return
  const node = readResearch(rec) || { parent: null, root: false, name: null, requiresUnlock: false, planet: null, reqs: [], objectives: [] }
  node.parent = target.entry.key
  node.root = false
  setResearch(rec, node)
}

const editor = reactive({ mode: 'parent', parent: '', name: '', requiresUnlock: false, planet: '', reqs: [], objectives: [] })
const hasResearch = computed(() => pickedRecord.value && readResearch(pickedRecord.value) != null)
const candidateOptions = computed(() => {
  const list = techCandidates().map((c) => ({ value: c, label: c }))
  if (editor.parent && !list.some((o) => o.value === editor.parent)) list.unshift({ value: editor.parent, label: editor.parent + " (?)" })
  return list
})
const objectiveTypeOptions = OBJECTIVE_TYPES.map((t) => ({ value: t, label: t }))
function refillEditor() {
  const r = pickedRecord.value
  if (!r) return
  const n = readResearch(r)
  editor.mode = n?.root ? 'root' : 'parent'
  editor.parent = n?.parent || ''
  editor.name = n?.name || ''
  editor.requiresUnlock = !!n?.requiresUnlock
  editor.planet = n?.planet || ''
  editor.reqs = [...(n?.reqs || [])]
  editor.objectives = (n?.objectives || []).map((o) => ({ type: o.type || 'Research', target: objectiveTarget(o) }))
}
watch(pickedId, refillEditor)
function commitResearch() {
  const r = pickedRecord.value
  if (!r) return
  const objectives = editor.objectives
    .filter((o) => o.type)
    .map((o) => {
      const obj = { type: o.type }
      if (o.target && o.target.trim()) obj[objectiveTargetKey(o.type)] = o.target.trim()
      return obj
    })
  setResearch(r, {
    root: editor.mode === 'root',
    parent: editor.mode === 'parent' && editor.parent ? editor.parent : null,
    name: editor.mode === 'root' && editor.name ? editor.name : null,
    requiresUnlock: editor.mode === 'root' && editor.requiresUnlock,
    planet: editor.planet || null,
    reqs: editor.reqs.filter((x) => x && x.trim()),
    objectives,
  })
}
function makeRoot() {
  const r = pickedRecord.value
  if (!r) return
  const n = readResearch(r) || { parent: null, root: false, name: null, requiresUnlock: false, planet: null, reqs: [], objectives: [] }
  n.root = true
  n.parent = null
  setResearch(r, n)
  editor.mode = 'root'
}
function removeResearchTree() {
  const r = pickedRecord.value
  if (!r) return
  setField(r, 'research', undefined)
}
function pickUnassigned(r) {
  pickedId.value = r.id
  select(r)
}
// enter+blur 双提交判重：值未变不赋值、不提交（与 fieldDrafts.commitDraft 同口径）。
function setEditorText(key, value) {
  if (editor[key] === value) return
  editor[key] = value
  commitResearch()
}
function setReqText(i, value) {
  if (editor.reqs[i] === value) return
  editor.reqs[i] = value
  commitResearch()
}
function setObjectiveTarget(i, value) {
  if (editor.objectives[i].target === value) return
  editor.objectives[i].target = value
  commitResearch()
}
// 加载失败的贴图按 URL 记入本地集合回退字母块；改 prop 派生对象无效，
// 且重新导入产生新 dataUrl 后自然恢复显示。
const failedSprites = reactive(new Set())
function techNodeColor(n) {
  return n.entry.record ? TYPE_COLOR[n.entry.record.type] || '#6B7A8A' : '#3A4652'
}
function techNodeSprite(n) {
  if (!n.entry.record) return null
  const url = spriteUrl(n.entry.record)
  return url && !failedSprites.has(url) ? url : null
}
function onNodeSpriteError(n) {
  const url = n.entry.record ? spriteUrl(n.entry.record) : null
  if (url) failedSprites.add(url)
}
</script>

<template>
  <div class="tech">
    <div class="tech-rail">
      <div class="cosmos-rail-head">{{ tr('tech.rail') }}</div>
      <div class="cosmos-search">
        <ArInput size="sm" :model-value="techSearch" :placeholder="tr('tech.search')" @input="techSearch = $event.target.value" />
      </div>
      <div class="tech-rail-list">
        <button class="tech-root-row" :class="{ active: techRoot === null }" @click="techRoot = null">{{ tr('tech.all') }}</button>
        <button v-for="(root, i) in filteredRoots" :key="i" class="tech-root-row" :class="{ active: techRoot === i }" @click="techRoot = i">
          <span class="tech-root-label">{{ root.label }}</span>
          <ArBadge :variant="root.problem ? 'warn' : 'neutral'">{{ root.problem ? tr('tech.problem.' + root.problem) : root.children.length }}</ArBadge>
        </button>
        <div class="tech-divider">{{ tr('tech.unassigned') }}</div>
        <button v-for="r in filteredUnassigned" :key="r.id" class="tech-root-row" :class="{ active: pickedId === r.id }" @click="pickUnassigned(r)">
          <span class="tech-root-label">{{ displayName(r) }}</span>
          <ArBadge variant="neutral">{{ r.type }}</ArBadge>
        </button>
      </div>
      <div class="cosmos-actions">
        <ArButton variant="soft" size="sm" block :disabled="!pickedRecord || hasResearch" @click="makeRoot">{{ tr('tech.makeRoot') }}</ArButton>
        <ArButton variant="danger" size="sm" block :disabled="!pickedRecord || !hasResearch" @click="removeResearchTree">{{ tr('tech.remove') }}</ArButton>
      </div>
    </div>
    <div ref="canvasRef" class="tech-canvas" @wheel.prevent="onTechWheel" @pointerdown="onTechPanStart">
      <div class="tech-world" :style="worldStyle">
        <svg class="tech-edges" width="1" height="1" style="overflow: visible">
          <path v-for="(e, i) in techWorld.edges" :key="i" :d="'M ' + e.x1 + ' ' + e.y1 + ' V ' + (e.y1 + e.y2) / 2 + ' H ' + e.x2 + ' V ' + e.y2" />
          <line v-if="rubber" :x1="rubber.sx" :y1="rubber.sy" :x2="rubber.ex" :y2="rubber.ey" class="tech-rubber" />
        </svg>
        <div
          v-for="n in techWorld.nodes" :key="n.entry.key + ':' + (n.entry.recordId || 'v')"
          class="tech-node" :class="{ picked: n.entry.recordId === pickedId, problem: !!n.entry.problem, vanilla: n.entry.vanilla }"
          :style="{ left: n.x + 'px', top: n.y + 'px' }"
          @pointerdown.stop @click.stop="pickNode(n)">
          <span class="tech-node-face" :style="{ background: techNodeColor(n) }">
            <img v-if="techNodeSprite(n)" :src="techNodeSprite(n)" alt="" @error="onNodeSpriteError(n)" />
            <span v-else class="tech-node-letter">{{ n.entry.label.slice(0, 1) }}</span>
          </span>
          <span v-if="techZoom > 0.4" class="tech-node-label">{{ n.entry.label }}</span>
          <span v-if="n.entry.record" class="tech-port" title="拖动改父级" @pointerdown.stop="portDown(n, $event)"></span>
        </div>
      </div>
      <div class="tech-tools">
        <ArButton variant="ghost" size="xs" icon @click="zoomBy(1 / 1.2)">−</ArButton>
        <span class="tech-zoom">{{ Math.round(techZoom * 100) }}%</span>
        <ArButton variant="ghost" size="xs" icon @click="zoomBy(1.2)">+</ArButton>
        <ArButton variant="soft" size="xs" @click="fitTech">{{ tr('tech.zoomFit') }}</ArButton>
      </div>
      <div class="tech-hint">{{ tr('tech.hint') }}</div>
    </div>
    <div class="tech-editor">
      <template v-if="pickedRecord">
        <div class="tech-ed-head">
          <p class="tech-ed-title">{{ displayName(pickedRecord) }}</p>
          <ArBadge variant="neutral">{{ pickedRecord.type }}</ArBadge>
        </div>
        <template v-if="hasResearch">
          <ArTabs
            size="sm" block
            :tabs="[{ key: 'parent', label: tr('tech.mode.parent') }, { key: 'root', label: tr('tech.mode.root') }]"
            :model-value="editor.mode"
            @update:model-value="editor.mode = $event; commitResearch()" />
          <div v-if="editor.mode === 'parent'" class="fld-ar-row">
            <div class="fld-ar-label">{{ tr('tech.parent') }}</div>
            <ArSelect size="sm" :options="candidateOptions" :model-value="editor.parent" @update:model-value="editor.parent = $event; commitResearch()" />
          </div>
          <template v-else>
            <div class="fld-ar-row">
              <div class="fld-ar-label">{{ tr('tech.rootName') }}</div>
              <ArInput size="sm" :model-value="editor.name" @enter="setEditorText('name', $event.target.value)" @blur="setEditorText('name', $event.target.value)" />
            </div>
            <div class="fld-ar-row">
              <ArSwitch size="sm" :label="tr('tech.requiresUnlock')" :model-value="editor.requiresUnlock" @update:model-value="editor.requiresUnlock = $event; commitResearch()" />
            </div>
          </template>
          <div class="fld-ar-row">
            <div class="fld-ar-label">{{ tr('tech.planet') }}</div>
            <ArInput size="sm" :model-value="editor.planet" @enter="setEditorText('planet', $event.target.value)" @blur="setEditorText('planet', $event.target.value)" />
          </div>
          <p class="insp-group">{{ tr('tech.reqs') }}</p>
          <div class="fld-row" v-for="(req, i) in editor.reqs" :key="'r' + i">
            <ArInput size="sm" :model-value="req" @enter="setReqText(i, $event.target.value)" @blur="setReqText(i, $event.target.value)" />
            <ArButton variant="ghost" size="xs" icon @click="editor.reqs.splice(i, 1); commitResearch()">×</ArButton>
          </div>
          <ArButton variant="soft" size="xs" @click="editor.reqs.push(''); commitResearch()">{{ tr('tech.add') }}</ArButton>
          <p class="insp-group">{{ tr('tech.objectives') }}</p>
          <div class="tech-obj-row" v-for="(o, i) in editor.objectives" :key="'o' + i">
            <ArSelect size="sm" :options="objectiveTypeOptions" :model-value="o.type" @update:model-value="o.type = $event; commitResearch()" />
            <ArInput size="sm" :model-value="o.target" @enter="setObjectiveTarget(i, $event.target.value)" @blur="setObjectiveTarget(i, $event.target.value)" />
            <ArButton variant="ghost" size="xs" icon @click="editor.objectives.splice(i, 1); commitResearch()">×</ArButton>
          </div>
          <ArButton variant="soft" size="xs" @click="editor.objectives.push({ type: 'Research', target: '' }); commitResearch()">{{ tr('tech.add') }}</ArButton>
        </template>
        <template v-else>
          <p class="grid-empty">{{ tr('tech.notInTree') }}</p>
          <ArSelect size="sm" :options="candidateOptions" :model-value="editor.parent" @update:model-value="editor.parent = $event; commitResearch()" />
          <ArButton variant="soft" size="sm" block @click="makeRoot">{{ tr('tech.makeRoot') }}</ArButton>
        </template>
      </template>
      <p v-else class="grid-empty">{{ tr('inspector.empty') }}</p>
    </div>
  </div>
</template>

<style scoped src="./tech.css"></style>
