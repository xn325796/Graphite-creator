<script setup>
import { ref, reactive, computed, watch, onMounted, onUnmounted } from 'vue'
import {
  S, tr, refreshBoard, canvasSelectNode, canvasMove, canvasConnect, canvasDisconnect,
  canvasInsertReroute, canvasMoveReroute, canvasRemoveReroute, canvasDeleteNodes,
} from '../../store/project.js'
import { cardSize, portAnchor, portWorld, edgeGeometry, CONTENT } from '../../lib/canvasGeometry.js'
import ArButton from '../ui/ArButton.vue'
import ArTabs from '../ui/ArTabs.vue'

const boardRef = ref(null)
const cv = reactive({ panX: 40, panY: 40, zoom: 1 })
const cvSel = ref([])
const cvEdgeSel = ref(null)
const cvDragState = ref(null)
const cvPending = ref(null)
const cvRerouteDrag = ref(null)
let cvInited = null

const cvNodes = computed(() => {
  const board = S.board
  if (!board) return []
  const drag = cvDragState.value
  return board.nodes
    .map((node, i) => ({ node, card: board.cards[i] || null }))
    .filter((item) => item.card)
    .map((item) => {
      const size = cardSize(item.card)
      const dragOn = drag?.mode === 'cards' && drag.ids.includes(item.node.id)
      const dx = dragOn ? drag.delta[0] : 0
      const dy = dragOn ? drag.delta[1] : 0
      return {
        node: item.node,
        card: item.card,
        size,
        x: item.node.x + dx,
        y: item.node.y + dy,
        selected: cvSel.value.includes(item.node.id),
      }
    })
})

const cvCardsById = computed(() => {
  const map = new Map()
  for (const item of cvNodes.value) map.set(item.node.id, item)
  return map
})

const cvEdges = computed(() => {
  const board = S.board
  if (!board) return []
  const drag = cvRerouteDrag.value
  return board.edges.map((edge) => {
    const from = cvCardsById.value.get(edge.from)
    const to = cvCardsById.value.get(edge.to)
    const start = from ? portWorld(from, from.card, null) : { x: 0, y: 0 }
    const end = to ? portWorld(to, to.card, edge.fieldKey) : { x: 0, y: 0 }
    const moving = drag && drag.edgeId === edge.id
    const reroutes = edge.reroutes.map((point, i) =>
      moving && i === drag.index ? [drag.x, drag.y] : point)
    const geo = edgeGeometry({ ...edge, reroutes }, board.edgeStyle, start, end)
    return { edge, geo, reroutes, selected: cvEdgeSel.value === edge.id }
  })
})

const cvPendingPath = computed(() => {
  const p = cvPending.value
  if (!p) return ''
  const midX = p.sx + (p.ex - p.sx) * 0.5
  return `M ${p.sx} ${p.sy} C ${midX} ${p.sy}, ${midX} ${p.ey}, ${p.ex} ${p.ey}`
})

const cvBoardTabs = computed(() =>
  (S.board?.boards || []).map((b) => ({ key: String(b.index), label: b.name })))
const cvBoardKey = computed(() => (S.board ? String(S.board.board ?? '') : ''))

watch(() => S.board, (board) => {
  if (!board) { cvInited = null; return }
  const key = `${S.project?.name}:${board.board}`
  if (cvInited === key) return
  cvInited = key
  cvSel.value = []
  cvEdgeSel.value = null
  if (board.viewport) {
    cv.panX = board.viewport.x
    cv.panY = board.viewport.y
    cv.zoom = board.viewport.zoom > 0 ? board.viewport.zoom : 1
  } else {
    cv.panX = 40
    cv.panY = 40
    cv.zoom = 1
  }
}, { immediate: true })

function cvPoint(ev) {
  const rect = boardRef.value?.getBoundingClientRect()
  if (!rect) return { x: 0, y: 0 }
  return { x: (ev.clientX - rect.left - cv.panX) / cv.zoom, y: (ev.clientY - rect.top - cv.panY) / cv.zoom }
}

function cvZoomBy(factor) {
  cv.zoom = Math.min(2.5, Math.max(0.2, cv.zoom * factor))
}

function cvOnWheel(ev) {
  const rect = boardRef.value?.getBoundingClientRect()
  if (!rect) return
  const mx = ev.clientX - rect.left
  const my = ev.clientY - rect.top
  const factor = ev.deltaY < 0 ? 1.12 : 1 / 1.12
  const next = Math.min(2.5, Math.max(0.2, cv.zoom * factor))
  if (next === cv.zoom) return
  cv.panX = mx - ((mx - cv.panX) * next) / cv.zoom
  cv.panY = my - ((my - cv.panY) * next) / cv.zoom
  cv.zoom = next
}

function cvOnBackgroundDown(ev) {
  if (ev.button !== 0 && ev.button !== 1) return
  cvSel.value = []
  cvEdgeSel.value = null
  cvDragState.value = { mode: 'pan', sx: ev.clientX, sy: ev.clientY, px: cv.panX, py: cv.panY }
  window.addEventListener('pointermove', cvOnPointerMove)
  window.addEventListener('pointerup', cvOnPointerUp)
}

function cvCardDown(item, ev) {
  ev.stopPropagation()
  if (ev.button !== 0) return
  const id = item.node.id
  if (ev.shiftKey || ev.ctrlKey || ev.metaKey) {
    cvSel.value = cvSel.value.includes(id) ? cvSel.value.filter((x) => x !== id) : [...cvSel.value, id]
  } else if (!cvSel.value.includes(id)) {
    cvSel.value = [id]
  }
  cvEdgeSel.value = null
  canvasSelectNode(item.node)
  const ids = [...cvSel.value]
  const before = {}
  for (const i of ids) {
    const it = cvCardsById.value.get(i)
    if (it) before[i] = [it.node.x, it.node.y]
  }
  cvDragState.value = { mode: 'cards', sx: ev.clientX, sy: ev.clientY, ids, before, delta: [0, 0] }
  window.addEventListener('pointermove', cvOnPointerMove)
  window.addEventListener('pointerup', cvOnPointerUp)
}

function cvPortDown(item, ev) {
  ev.stopPropagation()
  if (ev.button !== 0) return
  const p = cvPoint(ev)
  cvPending.value = { fromId: item.node.id, sx: p.x, sy: p.y, ex: p.x, ey: p.y }
  cvDragState.value = { mode: 'connect' }
  window.addEventListener('pointermove', cvOnPointerMove)
  window.addEventListener('pointerup', cvOnPointerUp)
}

function cvEdgeDown(wrap, ev) {
  ev.stopPropagation()
  cvEdgeSel.value = wrap.edge.id
  cvSel.value = []
}

function cvEdgeDbl(wrap, ev) {
  const p = cvPoint(ev)
  canvasInsertReroute(wrap.edge.id, p.x, p.y)
}

function cvDotDown(wrap, index, ev) {
  ev.stopPropagation()
  if (ev.button !== 0) return
  const point = wrap.reroutes[index]
  cvRerouteDrag.value = { edgeId: wrap.edge.id, index, x: point[0], y: point[1], before: [point[0], point[1]] }
  cvDragState.value = { mode: 'reroute' }
  window.addEventListener('pointermove', cvOnPointerMove)
  window.addEventListener('pointerup', cvOnPointerUp)
}

function cvDotRemove(wrap, index) {
  canvasRemoveReroute(wrap.edge.id, index)
}

function cvFindInputPort(world) {
  const tolerance = Math.max(10, 10 / cv.zoom)
  for (const item of cvNodes.value) {
    const rows = item.card.rows || []
    for (const row of rows) {
      if (!(item.card.inputFields || []).includes(row.name)) continue
      const anchor = portAnchor(item.card, row.name)
      if (!anchor) continue
      if (Math.hypot(item.x + anchor.x - world.x, item.y + anchor.y - world.y) <= tolerance) {
        return { item, fieldKey: row.name }
      }
    }
  }
  return null
}

function cvOnPointerMove(ev) {
  const state = cvDragState.value
  if (!state) return
  if (state.mode === 'pan') {
    cv.panX = state.px + (ev.clientX - state.sx)
    cv.panY = state.py + (ev.clientY - state.sy)
  } else if (state.mode === 'cards') {
    cvDragState.value = { ...state, delta: [(ev.clientX - state.sx) / cv.zoom, (ev.clientY - state.sy) / cv.zoom] }
  } else if (state.mode === 'connect' && cvPending.value) {
    const p = cvPoint(ev)
    cvPending.value = { ...cvPending.value, ex: p.x, ey: p.y }
  } else if (state.mode === 'reroute' && cvRerouteDrag.value) {
    const p = cvPoint(ev)
    cvRerouteDrag.value = { ...cvRerouteDrag.value, x: p.x, y: p.y }
  }
}

function cvOnPointerUp(ev) {
  window.removeEventListener('pointermove', cvOnPointerMove)
  window.removeEventListener('pointerup', cvOnPointerUp)
  const state = cvDragState.value
  cvDragState.value = null
  if (!state) return
  if (state.mode === 'cards') {
    const [dx, dy] = state.delta
    if (Math.abs(dx) > 0.5 || Math.abs(dy) > 0.5) {
      canvasMove(Object.entries(state.before).map(([id, before]) => ({
        id,
        before: [before[0], before[1]],
        after: [before[0] + dx, before[1] + dy],
      })))
    }
  } else if (state.mode === 'connect') {
    const pending = cvPending.value
    cvPending.value = null
    if (!pending) return
    const target = cvFindInputPort(cvPoint(ev))
    if (target && target.item.node.id !== pending.fromId) {
      canvasConnect(pending.fromId, target.item.node.id, target.fieldKey)
    }
  } else if (state.mode === 'reroute') {
    const drag = cvRerouteDrag.value
    cvRerouteDrag.value = null
    if (!drag) return
    if (Math.abs(drag.x - drag.before[0]) > 0.5 || Math.abs(drag.y - drag.before[1]) > 0.5) {
      canvasMoveReroute(drag.edgeId, drag.index, [drag.before[0], drag.before[1]], [drag.x, drag.y])
    }
  }
}

watch(() => S.ui.fitTick, () => fitBoard())

function fitBoard() {
  const items = cvNodes.value
  const rect = boardRef.value?.getBoundingClientRect()
  if (!items.length || !rect) return
  let minX = Infinity
  let minY = Infinity
  let maxX = -Infinity
  let maxY = -Infinity
  for (const item of items) {
    minX = Math.min(minX, item.x)
    minY = Math.min(minY, item.y)
    maxX = Math.max(maxX, item.x + item.size.w)
    maxY = Math.max(maxY, item.y + item.size.h)
  }
  const scale = Math.min((rect.width - 80) / Math.max(1, maxX - minX), (rect.height - 80) / Math.max(1, maxY - minY), 1.2)
  cv.zoom = Math.max(0.2, scale)
  cv.panX = (rect.width - (maxX - minX) * cv.zoom) / 2 - minX * cv.zoom
  cv.panY = (rect.height - (maxY - minY) * cv.zoom) / 2 - minY * cv.zoom
}

function onCvBoard(key) {
  refreshBoard(Number(key))
}

function cvDeleteSelection() {
  if (cvEdgeSel.value) {
    canvasDisconnect(cvEdgeSel.value)
    cvEdgeSel.value = null
    return
  }
  if (cvSel.value.length) {
    canvasDeleteNodes([...cvSel.value])
    cvSel.value = []
  }
}

function portStyle(card, fieldKey) {
  const anchor = portAnchor(card, fieldKey)
  if (!anchor) return { display: 'none' }
  return { left: `${anchor.x - CONTENT.PORT_R}px`, top: `${anchor.y - CONTENT.PORT_R}px` }
}

function onDeleteKey(e) {
  if (e.key !== 'Delete' && e.key !== 'Backspace') return
  const tag = (e.target?.tagName || '').toLowerCase()
  if (tag === 'input' || tag === 'textarea' || tag === 'select') return
  e.preventDefault()
  cvDeleteSelection()
}
onMounted(() => window.addEventListener('keydown', onDeleteKey))
onUnmounted(() => window.removeEventListener('keydown', onDeleteKey))
</script>

<template>
  <div class="cv">
    <div class="cv-toolbar">
      <ArTabs
        v-if="(S.board?.boards || []).length"
        size="sm" :tabs="cvBoardTabs" :model-value="cvBoardKey" @update:model-value="onCvBoard" />
      <span class="cv-style">{{ tr('canvas.style') }} · {{ S.board?.edgeStyle || 'bezier' }}</span>
      <div class="cv-tools">
        <ArButton variant="ghost" size="xs" icon @click="cvZoomBy(1 / 1.2)">−</ArButton>
        <span class="cv-zoom">{{ Math.round(cv.zoom * 100) }}%</span>
        <ArButton variant="ghost" size="xs" icon @click="cvZoomBy(1.2)">+</ArButton>
        <ArButton variant="soft" size="xs" @click="fitBoard">{{ tr('canvas.fit') }}</ArButton>
        <ArButton variant="danger" size="xs" :disabled="!cvEdgeSel && !cvSel.length" @click="cvDeleteSelection">{{ tr('canvas.delete') }}</ArButton>
      </div>
    </div>
    <div ref="boardRef" class="cv-canvas" :class="{ plain: !S.ui.grid }" @wheel.prevent="cvOnWheel" @pointerdown="cvOnBackgroundDown">
      <div class="cv-world" :style="{ transform: `translate(${cv.panX}px,${cv.panY}px) scale(${cv.zoom})` }">
        <svg class="cv-edges">
          <g v-for="wrap in cvEdges" :key="wrap.edge.id">
            <path class="cv-edge-pick" :d="wrap.geo.d" @pointerdown.stop="cvEdgeDown(wrap, $event)" @dblclick.stop="cvEdgeDbl(wrap, $event)" />
            <path class="cv-edge" :class="{ selected: wrap.selected }" :d="wrap.geo.d" />
            <path
              class="cv-arrow" :class="{ selected: wrap.selected }"
              d="M -8 -3.5 L 0 0 L -8 3.5 Z"
              :transform="`translate(${wrap.geo.arrow.x},${wrap.geo.arrow.y}) rotate(${wrap.geo.arrow.angleDeg})`" />
            <text v-if="wrap.edge.label" class="cv-edge-label" :x="wrap.geo.label.x" :y="wrap.geo.label.y + 8">{{ wrap.edge.label }}</text>
            <circle
              v-for="(point, di) in wrap.reroutes" :key="di"
              class="cv-dot" :cx="point[0]" :cy="point[1]" r="4"
              @pointerdown.stop="cvDotDown(wrap, di, $event)" @dblclick.stop="cvDotRemove(wrap, di)" />
          </g>
          <path v-if="cvPending" class="cv-pending" :d="cvPendingPath" />
        </svg>
        <div
          v-for="item in cvNodes" :key="item.node.id"
          class="cv-card" :class="{ selected: item.selected, carrier: item.card.carrier }"
          :style="{ left: item.x + 'px', top: item.y + 'px', width: item.size.w + 'px', height: item.size.h + 'px' }"
          :title="item.card.subtitle || ''"
          @pointerdown="cvCardDown(item, $event)">
          <div class="cv-head" :style="{ background: item.card.color }">
            <span class="cv-title">{{ item.card.title }}</span>
            <span v-if="item.card.typeLabel" class="cv-type">{{ item.card.typeLabel }}</span>
          </div>
          <template v-if="!item.card.carrier">
            <div
              v-for="(row, ri) in item.card.rows" :key="row.name"
              class="cv-row" :style="{ top: (CONTENT.HEADER + ri * CONTENT.ROW) + 'px' }">
              <span class="cv-row-label" :title="row.name">{{ row.name }}</span>
              <span class="cv-row-val">{{ row.text || '—' }}</span>
            </div>
            <span class="cv-port out" :style="portStyle(item.card, null)" title="ID" @pointerdown.stop="cvPortDown(item, $event)"></span>
            <span
              v-for="row in (item.card.rows || []).filter((r2) => (item.card.inputFields || []).includes(r2.name))"
              :key="'p' + row.name"
              class="cv-port in" :style="portStyle(item.card, row.name)" :title="row.name"></span>
          </template>
          <span v-else class="cv-port out" :style="portStyle(item.card, null)" title="ID" @pointerdown.stop="cvPortDown(item, $event)"></span>
        </div>
      </div>
      <p class="grid-empty cv-empty" v-if="!cvNodes.length">{{ tr('canvas.empty') }}</p>
    </div>
    <p class="cv-hint">{{ tr('canvas.hint') }}</p>
  </div>
</template>

<style scoped src="./canvas.css"></style>
