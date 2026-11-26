import { reactive } from 'vue'
import { S, setField, renameRecord, applyRaw, settleSelection, log, tr } from '../store/project.js'
import { toast } from '../lib/toast.js'

export const drafts = reactive({})
export const invalids = reactive(new Set())

export const draftKey = (r, f) => `${r.id}:${f}`

export function draft(r, f) {
  const k = draftKey(r, f)
  if (!(k in drafts)) {
    const v = r.fields[f]
    drafts[k] = v == null ? '' : typeof v === 'object' ? JSON.stringify(v) : String(v)
  }
  return drafts[k]
}

// 已提交值与待决重命名：enter+blur 双提交判重，避免一次编辑产生两条 undo。
const lastCommit = new Map()
const pendingNames = new Map()

export function clearDrafts() {
  for (const k of Object.keys(drafts)) delete drafts[k]
  invalids.clear()
  lastCommit.clear()
  pendingNames.clear()
}

const fieldText = (v) => (v == null ? '' : typeof v === 'object' ? JSON.stringify(v) : String(v))

// 击键只更新草稿，不提交命令（每条命令一条 undo，逐击键提交会刷爆撤销栈）。
export function onInput(r, f, ev) {
  drafts[draftKey(r, f.name)] = ev.target.value
}

// blur/enter/应用按钮统一走这里：值未变不提交；被拒（ok===false）回滚草稿到提交前值。
export async function commitDraft(r, f) {
  const k = draftKey(r, f.name)
  if (!(k in drafts)) return
  const v = drafts[k]
  const prevText = fieldText(r.fields[f.name])
  if (v === prevText || lastCommit.get(k) === v) { invalids.delete(k); return }
  lastCommit.set(k, v)
  const ok = await setField(r, f.name, v)
  if (ok === false) {
    invalids.add(k)
    drafts[k] = prevText
    lastCommit.delete(k)
  } else if (ok === true) {
    invalids.delete(k)
  }
}

export function isInvalid(r, f) {
  return invalids.has(draftKey(r, f.name))
}

export function applyDraft(r, f) {
  return commitDraft(r, f)
}

export function commitName(r, ev) {
  const value = ev.target.value.trim()
  if (value === r.name || pendingNames.get(r.id) === value) return
  const err = renameRecord(r, value)
  if (err) {
    log(err)
    toast(err, 'warn')
    ev.target.value = r.name
    return
  }
  clearDrafts()
  pendingNames.set(r.id, value)
}

export function toggleBool(r, f) {
  setField(r, f.name, !r.fields[f.name])
}

export function onRefChange(r, f, v) {
  setField(r, f.name, v || undefined)
}

export function toggleEditRaw(r) {
  S.ui.editRaw = !S.ui.editRaw
  if (S.ui.editRaw) S.ui.rawDraft = r.raw
}

export async function doApplyRaw(r) {
  if (await applyRaw(r, S.ui.rawDraft)) {
    await settleSelection()
    S.ui.editRaw = false
    clearDrafts()
    toast(tr('log.rawApplied'))
  }
}
