<script setup>
import { computed, reactive, ref, watch } from 'vue'
import {
  S, tr, selected, displayName, issuesFor, spriteUrl, recordId, pageCount,
  setPatch, setStringsFilter, setString, setType, select, errorCount, warnCount,
  locSummary, fieldSpecs, fieldDesc, candidatesFor, swatchFor, spriteImport, setClass,
} from '../../store/project.js'
import { TYPE_COLOR } from '../../store/schema.js'
import { issueText } from '../../store/validate.js'
import {
  draft, drafts, draftKey, isInvalid, onInput, commitDraft, applyDraft, commitName, toggleBool, onRefChange,
  toggleEditRaw, doApplyRaw,
} from '../../shared/fieldDrafts.js'
import ArButton from '../ui/ArButton.vue'
import ArInput from '../ui/ArInput.vue'
import ArSelect from '../ui/ArSelect.vue'
import ArSwitch from '../ui/ArSwitch.vue'
import ArTabs from '../ui/ArTabs.vue'
import ArBadge from '../ui/ArBadge.vue'
import PathPicker from '../layout/PathPicker.vue'

const sel = computed(() => selected())
const lang = computed(() => S.ui.lang)
const inspTabs = computed(() => [
  { key: 'props', label: tr('tab.props') },
  { key: 'preview', label: tr('tab.preview') },
  { key: 'validate', label: tr('tab.validate') },
  { key: 'text', label: tr('tab.text') },
])
function setTab(tp) {
  S.ui.inspectorTab = tp
  if (tp === 'props' && sel.value) S.ui.rawDraft = sel.value.raw
}
function jumpToIssue(i) {
  if (!i.contentId || !S.project) return
  const r = S.project.records.find((x) => recordId(x) === i.contentId)
  if (!r) return
  S.ui.workspace = 'assets'
  setType(r.type)
  select(r)
}
function prevPage() { if (S.ui.page > 0) S.ui.page-- }
function nextPage() { if (S.ui.page < pageCount() - 1) S.ui.page++ }
const pageDots = computed(() => {
  const n = pageCount()
  if (n <= 7) return Array.from({ length: n }, (_, i) => i)
  const cur = S.ui.page
  const set = new Set([0, n - 1, cur - 1, cur, cur + 1])
  return [...set].filter((p) => p >= 0 && p < n).sort((a, b) => a - b)
})
const stringKeys = computed(() => {
  if (!S.project) return []
  const f = S.ui.stringsFilter.trim().toLowerCase()
  return Object.keys(S.project.strings)
    .filter((k) => !f || k.toLowerCase().includes(f))
    .sort()
})
const missingCount = computed(() =>
  Object.values(S.project?.strings || {}).reduce(
    (acc, e) => acc + (e.zh ? 0 : 1) + (e.en ? 0 : 1),
    0,
  ),
)
function onStringEdit(key, l, ev) {
  setString(key, l, ev.target.value)
}
const previewStats = computed(() => {
  const r = sel.value
  if (!r) return []
  return fieldSpecs()
    .filter((f) => f.control === 'number' && r.fields[f.name] != null)
    .slice(0, 5)
    .map((f) => ({ label: fieldDesc(f.name) || f.name, n: f.name, v: r.fields[f.name] }))
})
const recordColor = (r) => {
  const c = r.fields?.color
  return typeof c === 'string' && c ? `#${c.replace(/^#/, '')}` : TYPE_COLOR[r.type]
}
const issueState = computed(() => {
  const list = sel.value ? issuesFor(sel.value) : []
  if (list.some((i) => i.level === 'error')) return 'err'
  if (list.length) return 'warn'
  return 'ok'
})
const issueLabel = computed(() => {
  const n = sel.value ? issuesFor(sel.value).length : 0
  return n ? tr('inspector.issues', { n }) : tr('inspector.noIssues')
})

// ---------- 类名行（有效类三态：declared / script / default） ----------
// 来源由后端 classSource 下发；只有 default 需要真正的编辑入口——脚本推断是只读事实，
// 已声明是现状。default 时给一个「指定类」入口，写入后由 selection 广播带回 declared。
const classSource = computed(() => S.selClassSource || 'declared')
const classEditing = ref(false)
const classDraft = ref('')
const classBusy = ref(false)

function toggleClassEdit() {
  if (classBusy.value) return
  classEditing.value = !classEditing.value
  classDraft.value = ''
}

function closeClassEdit() {
  if (classBusy.value) return
  classEditing.value = false
  classDraft.value = ''
}

async function applyClassEdit() {
  const r = sel.value
  if (!r || classBusy.value) return
  classBusy.value = true
  try {
    await setClass(r, classDraft.value)
    classEditing.value = false
    classDraft.value = ''
  } finally {
    classBusy.value = false
  }
}

// 换选中记录时收起编辑行，避免把上一条的草稿带过来。
watch(() => S.selectedId, () => {
  classEditing.value = false
  classDraft.value = ''
})

// ---------- 导入贴图（PathPicker 选源 PNG；内联记录没有独立贴图文件，隐藏入口） ----------

const spritePicker = reactive({ open: false, busy: false })
const spritePickerPath = ref('')
const canImportSprite = computed(() => !!sel.value && !sel.value.inline)
const spritePickerInitial = computed(() => {
  const p = S.project?.path
  return p && /[\\/]/.test(p) ? p : ''
})
function startSpriteImport() {
  if (!canImportSprite.value) return
  spritePickerPath.value = spritePickerInitial.value
  spritePicker.open = true
}
function closeSpritePicker() {
  spritePicker.open = false
  spritePicker.busy = false
}
async function onSpriteImportConfirm({ path }) {
  const r = sel.value
  if (!r) return
  spritePicker.busy = true
  try {
    if (await spriteImport(r.id, path)) closeSpritePicker()
  } finally {
    spritePicker.busy = false
  }
}
</script>

<template>
  <aside class="inspector">
    <div class="inspector-tabs">
      <ArTabs size="xs" block :tabs="inspTabs" :model-value="S.ui.inspectorTab" @update:model-value="setTab" />
    </div>

    <header class="inspector-head">
      <div class="inspector-title-row">
        <h2 class="inspector-title">{{ sel ? displayName(sel) : tr('inspector.empty') }}</h2>
        <ArBadge v-if="sel" variant="neutral">{{ sel.type }}</ArBadge>
      </div>
      <div class="inspector-sub" v-if="sel">
        <span class="inspector-id" :title="sel.name">{{ sel.name }}</span>
        <span class="inspector-state" :class="'state-' + issueState">{{ issueLabel }}</span>
      </div>
    </header>

    <div class="inspector-body" v-show="S.ui.inspectorTab === 'props'">
      <template v-if="sel">
        <p class="insp-group">{{ tr('inspector.identity') }}</p>
        <div class="field-row">
          <ArInput size="sm" data-inspector-name :label="tr('inspector.name')" :model-value="sel.name" @enter="commitName(sel, $event)" @blur="commitName(sel, $event)" />
        </div>
        <div class="field-row">
          <span class="field-label">{{ tr('inspector.class') }}</span>
          <div class="class-value" aria-live="polite">
            <span
              class="field-static mono class-name"
              :title="classSource === 'script'
                ? tr('inspector.classScriptHint')
                : classSource === 'default' ? tr('inspector.classDefaultHint') : tr('inspector.classDeclaredHint')">{{ S.selClassName || sel.type }}</span>
            <ArBadge v-if="classSource === 'script'" variant="accent" :title="tr('inspector.classScriptHint')">
              {{ tr('inspector.classFromScript') }}
            </ArBadge>
            <ArButton
              v-if="classSource === 'default'"
              class="class-toggle"
              :class="{ on: classEditing }"
              variant="ghost" size="xs" icon
              :aria-expanded="classEditing"
              :aria-label="tr('inspector.classPick')"
              :title="tr('inspector.classPick')"
              @click="toggleClassEdit">
              <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true" focusable="false">
                <path d="M11.1 2a1.5 1.5 0 0 1 2.1 0l.8.8a1.5 1.5 0 0 1 0 2.1l-7.6 7.6-3.5.9.9-3.5L11.1 2Z" fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" />
              </svg>
            </ArButton>
          </div>
        </div>
        <template v-if="classSource === 'default'">
          <div class="class-note">
            <ArBadge variant="warn" :title="tr('inspector.classDefaultHint')">{{ tr('inspector.classDefaultBadge', { cls: S.selClassName || sel.type }) }}</ArBadge>
          </div>
          <div v-if="classEditing" class="class-edit" @keydown.esc="closeClassEdit">
            <ArInput
              size="sm" mono
              :model-value="classDraft" :placeholder="tr('inspector.classPlaceholder')"
              :disabled="classBusy"
              @update:model-value="classDraft = $event"
              @enter="applyClassEdit" />
            <ArButton variant="soft" size="xs" :disabled="classBusy" @click="applyClassEdit">{{ tr('inspector.apply') }}</ArButton>
            <ArButton variant="ghost" size="xs" :disabled="classBusy" @click="closeClassEdit">{{ tr('dlg.cancel') }}</ArButton>
          </div>
          <p v-if="classEditing" class="class-hint">{{ tr('inspector.classClearHint') }}</p>
        </template>
        <div class="field-row">
          <span class="field-label">{{ tr('inspector.id') }}</span>
          <span class="field-static mono">{{ sel.name }}</span>
        </div>
        <div class="field-row">
          <ArSwitch size="sm" :label="tr('inspector.patch')" :model-value="!!sel.patch" @update:model-value="setPatch(sel, $event)" />
        </div>

        <p class="insp-group">{{ tr('inspector.localization') }}</p>
        <div class="loc-summary">
          <span>zh · {{ locSummary(sel).zh }}</span>
          <span>en · {{ locSummary(sel).en }}</span>
          <ArButton variant="ghost" size="xs" @click="setTab('text')">{{ tr('inspector.openStrings') }}</ArButton>
        </div>

        <p class="insp-group">{{ tr('inspector.fields') }}</p>
        <div class="field-row" v-for="f in fieldSpecs()" :key="f.name">
          <div class="field-label-col">
            <span class="field-label">{{ fieldDesc(f.name) || f.name }}</span>
            <span class="field-key">{{ f.name }}</span>
          </div>
          <ArSwitch
            v-if="f.control === 'bool'" size="sm"
            :model-value="!!sel.fields[f.name]"
            @update:model-value="toggleBool(sel, f)" />
          <div v-else-if="f.control === 'color'" class="field-color" :class="{ 'is-invalid': isInvalid(sel, f) }">
            <ArInput size="sm" mono :model-value="draft(sel, f.name)" @input="onInput(sel, f, $event)" @enter="commitDraft(sel, f)" @blur="commitDraft(sel, f)" />
            <span class="field-swatch" :style="{ background: swatchFor(f.name) }"></span>
          </div>
          <ArSelect
            v-else-if="f.control === 'content'" size="sm"
            :model-value="sel.fields[f.name] || ''"
            :options="[{ value: '', label: '—' }, ...candidatesFor(f.name).map((c) => ({ value: c, label: c }))]"
            @update:model-value="onRefChange(sel, f, $event)" />
          <div v-else-if="['raw', 'struct', 'stack', 'stackArray', 'enumeration', 'textArray', 'sprite'].includes(f.control)" class="field-apply">
            <ArInput size="sm" :invalid="isInvalid(sel, f)" :model-value="draft(sel, f.name)" @input="drafts[draftKey(sel, f.name)] = $event.target.value" />
            <ArButton variant="soft" size="xs" @click="applyDraft(sel, f)">{{ tr('inspector.apply') }}</ArButton>
          </div>
          <ArInput v-else size="sm" :invalid="isInvalid(sel, f)" :model-value="draft(sel, f.name)" @input="onInput(sel, f, $event)" @enter="commitDraft(sel, f)" @blur="commitDraft(sel, f)" />
        </div>

        <p class="insp-group">{{ tr('inspector.raw') }}</p>
        <div class="raw-head">
          <ArSwitch size="sm" :label="tr('inspector.rawEdit')" :model-value="S.ui.editRaw" @update:model-value="toggleEditRaw(sel)" />
          <ArButton v-if="S.ui.editRaw" variant="soft" size="xs" @click="doApplyRaw(sel)">{{ tr('inspector.apply') }}</ArButton>
        </div>
        <textarea class="raw-area" :readonly="!S.ui.editRaw" :value="S.ui.editRaw ? S.ui.rawDraft : sel.raw" @input="S.ui.rawDraft = $event.target.value"></textarea>
      </template>

      <div class="empty-state" v-else>
        <strong>{{ tr('inspector.empty') }}</strong>
        <span>{{ tr('inspector.emptyBody') }}</span>
      </div>
    </div>

    <div class="inspector-body" v-show="S.ui.inspectorTab === 'preview'">
      <template v-if="sel">
        <div class="preview-canvas">
          <img v-if="spriteUrl(sel)" class="preview-sprite" :src="spriteUrl(sel)" alt="" />
          <span v-else class="preview-swatch" :style="{ background: recordColor(sel) }"></span>
        </div>
        <p class="preview-name">{{ displayName(sel) }}</p>
        <p class="preview-id">{{ sel.type }} / {{ sel.name }}</p>
        <div class="preview-actions" v-if="canImportSprite">
          <ArButton variant="soft" size="sm" @click="startSpriteImport">{{ tr('inspector.importSprite') }}</ArButton>
        </div>
        <div class="preview-stats">
          <div class="preview-stat" v-for="st in previewStats" :key="st.n">
            <span class="preview-stat-label">{{ st.label }}</span><span class="preview-stat-value">{{ st.v }}</span>
          </div>
        </div>
        <div class="preview-palette">
          <span class="palette-chip" :style="{ background: recordColor(sel) }" :title="'record'"></span>
          <span class="palette-chip" :style="{ background: TYPE_COLOR[sel.type] }" :title="'type'"></span>
          <span class="palette-chip" style="background: #232b33" title="panel"></span>
        </div>
      </template>
      <div class="empty-state" v-else><strong>{{ tr('inspector.empty') }}</strong></div>
    </div>

    <div class="inspector-body" v-show="S.ui.inspectorTab === 'validate'">
      <div class="issues-summary">
        <span>{{ tr('issues.summary', { e: errorCount(), w: warnCount() }) }}</span>
        <ArBadge v-if="S.ui.validating" variant="accent">{{ tr('status.validating') }}</ArBadge>
      </div>
      <p class="issues-empty" v-if="!S.issues.length">{{ tr('issues.empty') }}</p>
      <button class="issue-row" v-for="(i, idx) in S.issues" :key="idx" :class="'issue-' + i.level" @click="jumpToIssue(i)">
        <span class="issue-level">{{ tr(i.level === 'error' ? 'issues.error' : 'issues.warn') }}</span>
        <span class="issue-text">{{ issueText(i, lang) }}</span>
      </button>
    </div>

    <div class="inspector-body" v-show="S.ui.inspectorTab === 'text'">
      <div class="strings-filter">
        <ArInput size="sm" :model-value="S.ui.stringsFilter" :placeholder="tr('strings.filter')" @input="setStringsFilter($event.target.value)" />
        <ArBadge :variant="missingCount ? 'warn' : 'accent'">{{ tr('strings.missing', { n: missingCount }) }}</ArBadge>
      </div>
      <table class="strings-table">
        <thead>
          <tr><th>{{ tr('strings.key') }}</th><th>简体中文</th><th>English</th></tr>
        </thead>
        <tbody>
          <tr v-for="k in stringKeys" :key="k">
            <td class="str-key" :title="k">{{ k }}</td>
            <td><input class="str-input" :class="{ 'str-missing': !S.project.strings[k].zh }" :value="S.project.strings[k].zh" @change="onStringEdit(k, 'zh', $event)" /></td>
            <td><input class="str-input" :class="{ 'str-missing': !S.project.strings[k].en }" :value="S.project.strings[k].en" @change="onStringEdit(k, 'en', $event)" /></td>
          </tr>
        </tbody>
      </table>
    </div>

    <footer class="inspector-pager" v-show="S.ui.workspace === 'assets' && pageCount() > 1">
      <ArButton variant="ghost" size="xs" icon :disabled="S.ui.page === 0" @click="prevPage">‹</ArButton>
      <button v-for="p in pageDots" :key="p" class="page-dot" :class="{ on: p === S.ui.page }" @click="S.ui.page = p"></button>
      <ArButton variant="ghost" size="xs" icon :disabled="S.ui.page >= pageCount() - 1" @click="nextPage">›</ArButton>
      <span class="page-num">{{ S.ui.page + 1 }} / {{ pageCount() }}</span>
    </footer>
  </aside>

  <PathPicker
    v-model:current="spritePickerPath"
    :open="spritePicker.open"
    mode="file"
    :filter="['.png']"
    :title="tr('picker.title.spriteSource')"
    :initial-path="spritePickerInitial"
    :confirm-label="tr('picker.confirmImport')"
    :busy="spritePicker.busy"
    @close="closeSpritePicker"
    @confirm="onSpriteImportConfirm">
    <p class="pp-sprite-note">{{ tr('picker.spriteTarget', { name: sel ? sel.name : '' }) }}</p>
    <p class="pp-sprite-note">{{ tr('picker.spriteContentHint') }}</p>
  </PathPicker>
</template>

<style scoped src="./inspector.css"></style>
<style scoped>
.pp-sprite-note { color: var(--text-3); font-size: var(--fs-xs); }
</style>
