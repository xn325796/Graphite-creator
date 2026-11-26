<script setup>
import { computed, ref, watch } from 'vue'
import { invoke, errMsg } from '../../lib/ipc.js'
import { tr } from '../../store/project.js'
import ArModal from '../ui/ArModal.vue'
import ArButton from '../ui/ArButton.vue'
import ArInput from '../ui/ArInput.vue'

const props = defineProps({
  open: Boolean,
  title: { type: String, default: '' },
  /** 'dir' 选目录 / 'file' 选文件（目录仍可双击进入） */
  mode: { type: String, default: 'dir' },
  /** 文件模式下的扩展名白名单，如 ['.zip', '.jar']；空表示不限 */
  filter: { type: Array, default: () => [] },
  initialPath: { type: String, default: '' },
  confirmLabel: { type: String, default: '' },
  confirmDisabled: Boolean,
  busy: Boolean,
})

const emit = defineEmits(['close', 'confirm'])
const current = defineModel('current', { type: String, default: '' })

const parent = ref(null)
const entries = ref([])
const selected = ref('')
const loading = ref(false)
const unavailable = ref('')

function norm(p) {
  let s = String(p || '').replace(/\\/g, '/').replace(/\/{2,}/g, '/')
  if (!s) return 'C:/'
  if (s.length > 1) s = s.replace(/\/+$/, '')
  if (/^[a-zA-Z]:$/.test(s)) s += '/'
  return s
}
function join(dir, name) {
  const d = norm(dir)
  return d.endsWith('/') ? d + name : d + '/' + name
}
const extOf = (name) => {
  const i = String(name || '').lastIndexOf('.')
  return i >= 0 ? name.slice(i).toLowerCase() : ''
}
const matchesFilter = (e) => !props.filter.length || props.filter.includes(extOf(e.name))

function defaultStart() {
  if (props.initialPath) return norm(props.initialPath)
  if (current.value) return norm(current.value)
  return 'C:/'
}

async function load(path) {
  const target = norm(path)
  current.value = target
  selected.value = ''
  loading.value = true
  unavailable.value = ''
  try {
    const res = await invoke('fs_list_dir', { path: target })
    parent.value = res?.parent ? norm(res.parent) : null
    const list = (res?.entries || []).map((e) => ({
      ...e,
      path: norm(e.path || join(res?.path || target, e.name)),
    }))
    list.sort((a, b) => (a.isDir === b.isDir ? a.name.localeCompare(b.name) : a.isDir ? -1 : 1))
    entries.value = list
  } catch (e) {
    entries.value = []
    parent.value = null
    unavailable.value = errMsg(e)
  } finally {
    loading.value = false
  }
}

watch(() => props.open, (v) => { if (v) load(defaultStart()) }, { immediate: true })
watch(() => props.initialPath, (v) => { if (props.open && v && !current.value) load(v) })

const visible = computed(() => props.mode === 'dir'
  ? entries.value.filter((e) => e.isDir)
  : entries.value.filter((e) => e.isDir || matchesFilter(e)))

const crumbs = computed(() => {
  const p = norm(current.value)
  const out = []
  if (/^[a-zA-Z]:\//.test(p)) {
    const parts = p.split('/')
    const drive = parts.shift()
    out.push({ label: drive + '/', path: drive + '/' })
    let acc = drive
    for (const part of parts.filter(Boolean)) {
      acc += '/' + part
      out.push({ label: part, path: acc })
    }
  } else {
    out.push({ label: '/', path: '/' })
    let acc = ''
    for (const part of p.split('/').filter(Boolean)) {
      acc += '/' + part
      out.push({ label: part, path: acc })
    }
  }
  return out
})

function pick(e) {
  if (e.isDir) {
    // 文件模式下目录只用于进入，不能被选中为结果
    if (props.mode === 'dir') selected.value = e.path
  } else if (props.mode === 'file' && matchesFilter(e)) {
    selected.value = e.path
  }
}
function openEntry(e) {
  if (e.isDir) load(e.path)
  else if (props.mode === 'file' && matchesFilter(e)) {
    selected.value = e.path
    confirmPick()
  }
}
function goUp() {
  if (parent.value) load(parent.value)
}
function reload() {
  load(current.value)
}

const chosen = computed(() => {
  if (selected.value) return selected.value
  if (props.mode === 'dir') return norm(current.value)
  const p = norm(current.value)
  // 手动输入兜底：必须看起来是带扩展名的文件，且命中白名单
  return /\.[^./]+$/.test(p) && matchesFilter({ name: p }) ? p : ''
})
const chosenName = computed(() => {
  const p = chosen.value
  if (!p) return ''
  return p.split('/').filter(Boolean).pop() || p
})
function confirmPick() {
  if (!chosen.value || props.confirmDisabled || props.busy) return
  emit('confirm', { path: chosen.value, name: chosenName.value })
}
function onManualEnter() {
  load(current.value)
}
</script>

<template>
  <ArModal :open="open" :title="title" :width="760" @close="emit('close')">
    <div class="pp">
      <div class="pp-bar">
        <ArButton variant="ghost" size="sm" :disabled="!parent" @click="goUp">{{ tr('picker.up') }}</ArButton>
        <div class="pp-crumbs">
          <template v-for="(c, i) in crumbs" :key="c.path">
            <button class="pp-crumb" :class="{ last: i === crumbs.length - 1 }" @click="load(c.path)">{{ c.label }}</button>
            <span v-if="i < crumbs.length - 1" class="pp-sep">/</span>
          </template>
        </div>
        <ArButton variant="ghost" size="sm" @click="reload">{{ tr('picker.refresh') }}</ArButton>
      </div>

      <div class="pp-path">
        <ArInput size="sm" v-model="current" :placeholder="tr('picker.path')" @input="selected = ''" @enter="onManualEnter" />
        <ArButton variant="soft" size="sm" @click="onManualEnter">{{ tr('picker.go') }}</ArButton>
      </div>

      <div class="pp-list">
        <p v-if="loading" class="pp-note">{{ tr('picker.loading') }}</p>
        <p v-else-if="unavailable" class="pp-error">{{ tr('picker.unavailable') }}<br /><span>{{ unavailable }}</span></p>
        <p v-else-if="!visible.length" class="pp-note">{{ tr('picker.empty') }}</p>
        <button
          v-for="e in visible" :key="e.path"
          class="pp-row" :class="{ sel: selected === e.path }"
          @click="pick(e)" @dblclick="openEntry(e)">
          <span class="pp-ico" :class="e.isDir ? 'dir' : 'file'">{{ e.isDir ? '▸' : '·' }}</span>
          <span class="pp-name">{{ e.name }}</span>
          <span v-if="e.isMod" class="pp-tag mod">{{ tr('picker.isMod') }}</span>
          <span v-if="e.isArchive" class="pp-tag zip">zip</span>
        </button>
      </div>

      <p class="pp-hint">{{ tr('picker.manualHint') }}</p>
      <slot />
    </div>

    <template #footer>
      <span v-if="chosenName" class="pp-chosen">{{ chosenName }}</span>
      <ArButton variant="soft" size="sm" @click="emit('close')">{{ tr('dlg.cancel') }}</ArButton>
      <ArButton variant="primary" size="sm" :disabled="!chosen || confirmDisabled || busy" @click="confirmPick">
        {{ confirmLabel || tr('picker.confirm') }}
      </ArButton>
    </template>
  </ArModal>
</template>

<style scoped>
.pp { display: flex; flex-direction: column; gap: var(--sp-3); width: 100%; }
.pp-bar { display: flex; align-items: center; gap: var(--sp-2); }

.pp-crumbs {
  flex: 1;
  min-width: 0;
  display: flex;
  align-items: center;
  gap: 1px;
  height: var(--h-control-sm);
  padding: 0 var(--sp-3);
  overflow: hidden;
  background: var(--bg-field);
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
}
.pp-crumb {
  max-width: 140px;
  padding: 0 var(--sp-1);
  overflow: hidden;
  border: 0;
  background: transparent;
  color: var(--text-2);
  font-family: inherit;
  font-size: var(--fs-xs);
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}
.pp-crumb:hover { color: var(--text); }
.pp-crumb.last { color: var(--accent); }
.pp-sep { color: var(--text-3); font-size: var(--fs-2xs); }

.pp-path { display: flex; align-items: center; gap: var(--sp-3); }
.pp-path > .ar-field { flex: 1; min-width: 0; }

.pp-list {
  height: min(300px, 40vh);
  display: flex;
  flex-direction: column;
  gap: 1px;
  padding: var(--sp-2);
  overflow-y: auto;
  background: var(--bg-field);
  border: 1px solid var(--border);
  border-radius: var(--r-md);
}
.pp-row {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  width: 100%;
  height: 30px;
  padding: 0 var(--sp-2);
  border: 1px solid transparent;
  border-radius: var(--r-sm);
  background: transparent;
  color: var(--text);
  font-family: inherit;
  font-size: var(--fs-sm);
  text-align: left;
  cursor: pointer;
}
.pp-row:hover { background: var(--bg-hover); }
.pp-row.sel { background: var(--accent-soft); border-color: var(--accent-strong); }
.pp-ico { flex: none; width: 14px; color: var(--text-3); font-size: var(--fs-sm); text-align: center; }
.pp-ico.dir { color: var(--accent); }
.pp-name { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.pp-tag {
  flex: none;
  padding: 0 var(--sp-2);
  border: 1px solid var(--border-strong);
  border-radius: var(--r-xs);
  color: var(--text-2);
  font-size: 9px;
  line-height: 15px;
}
.pp-tag.mod { border-color: var(--accent-line); color: var(--accent); }

.pp-hint, .pp-note { color: var(--text-3); font-size: var(--fs-xs); }
.pp-note { padding: var(--sp-5); text-align: center; }
.pp-error { padding: var(--sp-4); color: var(--warn); font-size: var(--fs-xs); line-height: var(--lh); }
.pp-error span { color: var(--text-3); font-size: var(--fs-2xs); word-break: break-all; }
.pp-chosen {
  margin-right: auto;
  align-self: center;
  color: var(--text-2);
  font-size: var(--fs-xs);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
</style>
