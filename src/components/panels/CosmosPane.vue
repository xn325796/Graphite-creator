<script setup>
import { computed, ref, watch } from 'vue'
import {
  S, tr, select, displayName, fieldSpecs, fieldDesc,
  cosmosTree, cosmosGroups, cosmosSelection, setCosmosTab, setCosmosSearch,
  reassignSector, addCosmos, sectorPlanet, refreshCosmosFields, cosmosScene,
} from '../../store/project.js'
import { TYPE_LABEL } from '../../store/schema.js'
import ArButton from '../ui/ArButton.vue'
import ArInput from '../ui/ArInput.vue'
import ArTabs from '../ui/ArTabs.vue'
import ArBadge from '../ui/ArBadge.vue'
import CosmosPreview from './CosmosPreview.vue'

const dragging = ref(null)
const dragOverKey = ref(null)
const typeLabel = (tp) => TYPE_LABEL[S.ui.lang][tp]
const cosmosSel = computed(() => cosmosSelection())
const cosmosTabs = computed(() => [
  { key: '0', label: tr('cosmos.tab.grid') },
  { key: '1', label: tr('cosmos.tab.appearance') },
  { key: '2', label: tr('cosmos.tab.system') },
])

// cosmos_scene(recordId)：Rust 未就绪时返回 null，保留空态、不报错。
const scene = ref(null)
let sceneSeq = 0
async function loadScene() {
  const record = cosmosSel.value
  if (!record) {
    scene.value = null
    return
  }
  const seq = ++sceneSeq
  const dto = await cosmosScene(record.id)
  if (seq !== sceneSeq) return
  scene.value = dto
}
watch(() => cosmosSel.value?.id, () => { scene.value = null; loadScene() }, { immediate: true })
watch(() => S.ui.cosmosTab, () => { if (!scene.value) loadScene() })
// 字段变更后刷新场景，但保留旧场景直到新数据返回，避免画布闪空
watch(() => S.ui.cosmosRev, loadScene)

const highlightId = computed(() => (cosmosSel.value?.type === 'sector' ? cosmosSel.value.id : null))

const sceneInfo = computed(() => {
  if (!cosmosSel.value) return tr('cosmos.none')
  const sc = scene.value
  if (!sc) return ''
  if (S.ui.cosmosTab === 0) {
    if (!sc.grid) return tr('cosmos.gridless')
    const size = sc.sectorSize ?? sc.grid.size ?? 0
    const capacity = sc.grid.capacity ?? size * size
    const occupied = (sc.grid.tiles || []).filter((t) => t.occupied || (t.sectorIds || []).length).length
    return tr('cosmos.summary', { c: capacity, o: occupied, s: size })
  }
  if (S.ui.cosmosTab === 1) {
    const layers = sc.appearance?.surface || []
    if (!sc.appearance || !layers.length) return tr('cosmos.noMesh')
    return tr('cosmos.appearanceInfo', { t: sc.appearance.meshType || 'SerpuloTerrain' })
  }
  const n = sc.system?.bodies?.length || 0
  return n ? tr('cosmos.systemInfo', { n }) : tr('cosmos.noBodies')
})

const previewLabel = computed(() => {
  const keys = ['cosmos.tab.grid', 'cosmos.tab.appearance', 'cosmos.tab.system']
  return tr(keys[S.ui.cosmosTab] || keys[0])
})

const cosmosRows = computed(() => {
  const r = cosmosSel.value
  if (!r) return []
  const rows = [['name', displayName(r)]]
  if (r.type === 'sector') rows.push([tr('cosmos.field.planet'), sectorPlanet(r)])
  else rows.push(['sectorSize', String(r.fields.sectorSize ?? '')])
  for (const f of fieldSpecs()) {
    if (f.name === 'name' || f.name === 'planet' || f.name === 'sectorSize') continue
    const lower = f.name.toLowerCase()
    if (lower.includes('mesh') || lower.includes('atmosphere')) continue
    const v = r.fields[f.name]
    if (v == null || v === '') continue
    rows.push([fieldDesc(f.name) || f.name, typeof v === 'object' ? '…' : String(v)])
  }
  return rows
})

function cosmosAlreadyOn(key) {
  const s = dragging.value
  if (!s) return true
  const p = sectorPlanet(s)
  const mod = S.project?.internalName || S.project?.name
  return p === key || p === `${mod}-${key}`
}
function onDragStart(s) {
  dragging.value = s
}
function onDragOver(g) {
  if (dragging.value?.type === 'sector' && !cosmosAlreadyOn(g.key)) dragOverKey.value = g.key
}
async function onDrop(g) {
  if (!dragging.value) return
  await reassignSector(dragging.value, g.key)
  await refreshCosmosFields()
  dragging.value = null
  dragOverKey.value = null
}
function openCosmos(type) {
  addCosmos(type)
}
</script>

<template>
  <div class="cosmos">
    <div class="cosmos-rail">
      <div class="cosmos-rail-head">{{ tr('cosmos.group') }}</div>
      <div class="cosmos-search">
        <ArInput size="sm" :model-value="S.ui.cosmosSearch" :placeholder="tr('cosmos.search')" @input="setCosmosSearch($event.target.value)" />
      </div>
      <div class="cosmos-tree">
        <template v-for="g in cosmosTree()" :key="g.key">
          <div
            class="cosmos-group" :class="{ 'cosmos-drop': dragOverKey === g.key }"
            @dragover.prevent="onDragOver(g)" @dragleave="dragOverKey = null" @drop.prevent="onDrop(g)"
            @click="g.planet && select(g.planet)">
            <svg class="cosmos-ico" viewBox="0 0 24 24" aria-hidden="true">
              <circle cx="12" cy="12" r="9" />
              <path d="M3 12h18M12 3a15 15 0 0 1 0 18M12 3a15 15 0 0 0 0 18" />
            </svg>
            <span class="cosmos-group-label">{{ g.planet ? displayName(g.planet) : tr('cosmos.vanilla') + ' · ' + g.key }}</span>
            <ArBadge variant="neutral">{{ g.sectors.length }}</ArBadge>
          </div>
          <div
            v-for="s in g.sectors" :key="s.id"
            class="cosmos-sector" :class="{ 'is-selected': S.selectedId === s.id }"
            draggable="true"
            @dragstart="onDragStart(s)" @dragend="dragging = null; dragOverKey = null"
            @click="select(s)">
            <svg class="cosmos-ico" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 3l7 4v9l-7 4-7-4V7z" /></svg>
            <span>{{ displayName(s) }}</span>
          </div>
        </template>
        <p class="grid-empty" v-if="!cosmosGroups().length">{{ tr('cosmos.empty') }}</p>
      </div>
      <div class="cosmos-actions">
        <ArButton variant="soft" size="sm" block @click="openCosmos('planet')">{{ tr('cosmos.add.planet') }}</ArButton>
        <ArButton variant="soft" size="sm" block @click="openCosmos('sector')">{{ tr('cosmos.add.sector') }}</ArButton>
      </div>
    </div>
    <div class="cosmos-fields">
      <template v-if="cosmosSel">
        <p class="cosmos-type">{{ typeLabel(cosmosSel.type) }}</p>
        <div class="cosmos-field-row" v-for="(row, i) in cosmosRows" :key="i">
          <span class="cosmos-fk">{{ row[0] }}</span>
          <span class="cosmos-fv">{{ row[1] }}</span>
        </div>
      </template>
      <p class="grid-empty" v-else>{{ tr('cosmos.none') }}</p>
    </div>
    <div class="cosmos-preview">
      <div class="cosmos-preview-head">
        <ArTabs size="sm" :tabs="cosmosTabs" :model-value="String(S.ui.cosmosTab)" @update:model-value="setCosmosTab(Number($event))" />
      </div>
      <div class="cosmos-preview-body" v-if="cosmosSel">
        <CosmosPreview
          v-if="scene"
          :scene="scene"
          :tab="S.ui.cosmosTab"
          :highlight-id="highlightId"
          :label="previewLabel"
          :vanilla-label="tr('cosmos.vanilla')" />
        <div class="cosmos-stage cosmos-stage-empty" v-else>
          <p>{{ tr('cosmos.sceneUnavailable') }}</p>
        </div>
        <p class="cosmos-info" v-if="sceneInfo">{{ sceneInfo }}</p>
        <p class="cosmos-hint">{{ tr('cosmos.hint') }}</p>
      </div>
      <div class="cosmos-preview-body cosmos-empty" v-else>
        <p>{{ tr('cosmos.none') }}</p>
      </div>
    </div>
  </div>
</template>

<style scoped src="./cosmos.css"></style>
