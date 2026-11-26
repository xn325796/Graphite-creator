import { reactive } from 'vue'
import { invoke, listen, errCode, errMsg, isNoProject } from '../lib/ipc.js'
import { toast } from '../lib/toast.js'
import { TYPES, isVanilla, vanillaNames } from './schema.js'
import { t } from './messages.js'


const NAME_RE = /^[a-z0-9]+(-[a-z0-9]+)*$/

export const S = reactive({
  project: null,
  ui: {
    workspace: 'assets',
    type: 'item',
    page: 0,
    filter: '',
    view: 'grid',
    inspectorTab: 'props',
    search: '',
    stringsFilter: '',
    cosmosTab: 0,
    cosmosSearch: '',
    lang: 'zh',
    grid: true,
    snap: false,
    logOpen: false,
    menu: null,
    modal: null,
    session: null,
    gameRunning: false,
    closed: false,
    editRaw: false,
    rawDraft: '',
    validating: false,
    importing: null,
    showSprites: true,
    fitTick: 0,
    cosmosRev: 0,
    minimal: false,
  },
  selectedId: null,
  selRecord: null,
  selSchema: [],
  selClassName: null,
  // 有效类来源："declared" | "script" | "default"（后端 R-02 契约，见 mm_app::class_resolution）。
  // 旧载荷没有这一格时按 "declared" 兜底，检查器就不会对老数据误挂「由脚本推断」。
  selClassSource: 'declared',
  undoState: { canUndo: false, canRedo: false, undoDepth: 0, dirty: false },
  issues: [],
  logs: [],
  board: null,
  tech: { roots: [], unassigned: [], refused: [] },
  dirty: false,
  savedAt: null,
  savedKind: 'save',
  searchIdx: -1,
})

export const tr = (key, args = undefined) => t(key, S.ui.lang, args)

export function log(msg) {
  const time = new Date().toTimeString().slice(0, 8)
  S.logs.push({ time, msg })
  if (S.logs.length > 500) S.logs.shift()
}

// 命令失败必须让用户看见：日志之外再弹 toast；同命令+code+message 3 秒内只弹一次，
// 避免刷新链路（Promise.all 多命令）连环失败刷屏；跨命令同文错误互不吞。
const ERROR_TOAST_DEDUP_MS = 3000
const lastErrorToastAt = new Map()

function toastCommandError(e, cmd) {
  const key = `${cmd}|${errCode(e)}|${errMsg(e)}`
  const now = Date.now()
  for (const [k, at] of lastErrorToastAt) {
    if (now - at > ERROR_TOAST_DEDUP_MS) lastErrorToastAt.delete(k)
  }
  if (lastErrorToastAt.has(key)) return
  lastErrorToastAt.set(key, now)
  toast(tr('toast.cmdFailed', { msg: errMsg(e) }), 'err', 6000)
}

async function call(cmd, args) {
  try {
    return await invoke(cmd, args)
  } catch (e) {
    // no-project 保持不弹 toast 的原口径，但至少留一条日志可查。
    log(`[${errCode(e)}] ${errMsg(e)}`)
    if (!isNoProject(e)) toastCommandError(e, cmd)
    return null
  }
}


// ---------- 物化 ----------

const localeId = (l) => {
  const locales = S.project?.locales || []
  if (l === 'zh') return locales.find((x) => x.startsWith('zh')) || 'zh_CN'
  return locales.find((x) => x === 'en') || 'en'
}

function materializeStrings(dto) {
  const strings = {}
  for (const [key, pairs] of dto?.entries || []) {
    const by = Object.fromEntries(pairs)
    strings[key] = { zh: by[localeId('zh')] || '', en: by[localeId('en')] || '' }
  }
  return strings
}

function materializeSummary(summary) {
  if (!summary) return
  const prev = S.project
  S.project = {
    name: summary.internalName,
    displayName: summary.displayName,
    path: summary.root || '(未保存)',
    meta: {
      name: summary.meta?.name || '',
      displayName: summary.meta?.displayName || '',
      minGameVersion: summary.meta?.minGameVersion || '',
      description: summary.meta?.description || '',
      author: summary.meta?.author || '',
      version: summary.meta?.version || '',
    },
    records: prev?.records || [],
    strings: prev?.strings || {},
    locales: summary.locales || [],
    contentCount: summary.contentCount,
    assetCount: summary.contentCount,
  }
  S.dirty = summary.dirty
}

function materializeContents(items) {
  if (!S.project) return
  S.project.records = (items || []).map((it) => ({
    id: it.id,
    type: it.contentType,
    name: it.name,
    displayName: it.displayName,
    patch: it.patch,
    inline: it.inline,
    dirty: it.dirty,
    fields: { color: it.color && it.color !== '#ffffff' ? it.color.slice(1) : undefined },
  }))
}

function applyHistory(h) {
  if (!h) return
  S.undoState = { canUndo: h.canUndo, canRedo: h.canRedo, undoDepth: h.undoDepth, dirty: h.dirty }
  S.dirty = h.dirty
}

function typedFields(schema, pairs) {
  const fields = {}
  const specs = Object.fromEntries((schema?.fields || []).map((f) => [f.name, f]))
  for (const [k, v] of pairs || []) {
    const spec = specs[k]
    if (spec?.control === 'bool') fields[k] = spec.checked
    else if (spec?.control === 'number') fields[k] = spec.text === '' ? undefined : Number(spec.text)
    else fields[k] = typeof v === 'string' ? v : JSON.stringify(v)
  }
  return fields
}

let selSeq = 0
let selPending = Promise.resolve()
export const settleSelection = () => selPending
async function applySelection(payload) {
  const seq = ++selSeq
  const run = async () => {
    if (!payload || !payload.id) {
      S.selRecord = null
      S.selSchema = []
      S.selClassName = null
      S.selClassSource = 'declared'
      S.selectedId = null
      return
    }
    const id = payload.id
    S.selectedId = id
    const [schema, raw] = await Promise.all([
      call('record_schema', { recordId: id }),
      call('record_raw', { recordId: id }),
    ])
    if (seq !== selSeq) return
    const fields = schema ? typedFields(schema, payload.fields) : {}
    S.selRecord = {
      id,
      type: payload.contentType,
      name: payload.name,
      patch: payload.patch,
      inline: payload.inline,
      dirty: payload.dirty,
      fields,
      raw: raw ?? JSON.stringify({ type: payload.contentType, name: payload.name, ...fields }, null, 2),
    }
    S.selSchema = schema?.fields || []
    S.selClassName = schema?.className || payload.contentType
    S.selClassSource = schema?.classSource || 'declared'
    S.ui.rawDraft = S.selRecord.raw
  }
  selPending = run()
  return selPending
}

export const fieldSpecs = () => S.selSchema.filter((f) => f.shown)
export const fieldDesc = (n) => S.selSchema.find((f) => f.name === n)?.description || null
export const candidatesFor = (n) => S.selSchema.find((f) => f.name === n)?.candidates || []
export const swatchFor = (n) => S.selSchema.find((f) => f.name === n)?.swatch || '#ffffff'


// ---------- 刷新链路（事件驱动 + 合并去重） ----------

let refreshTask = null
let refreshAgain = false
// 合并去重：并发调用共享同一趟在途刷新（返回其 Promise），
// 否则 await 方会在数据尚未物化时提前返回（openDemo 日志「0 条记录」竞态根因）。
function refreshProject() {
  if (refreshTask) { refreshAgain = true; return refreshTask }
  refreshTask = (async () => {
    try {
      const [summary, items, history, strings] = await Promise.all([
        call('project_summary'),
        call('project_contents'),
        call('history_state'),
        call('project_strings'),
      ])
      if (summary && summary.internalName) {
        materializeSummary(summary)
        materializeContents(items)
        S.project.strings = materializeStrings(strings)
        // 项目内容变化 → 星球场景（网格/外观/轨道）失效，Cosmos 预览按 rev 重拉。
        clearCosmosScenes()
        S.ui.cosmosRev++
      } else if (!summary) {
        S.project = null
        S.selRecord = null
        S.selectedId = null
        S.tech = { roots: [], unassigned: [], refused: [] }
        S.board = null
        clearSprites()
        clearCosmosScenes()
      }
      applyHistory(history)
    } finally {
      refreshTask = null
      if (refreshAgain) { refreshAgain = false; refreshProject() }
      if (S.ui.workspace === 'tech') refreshTechFields()
      if (S.ui.workspace === 'canvas') refreshBoard()
    }
  })()
  return refreshTask
}

function mapIssues(issues) {
  return (issues || []).map((i) => ({
    level: i.level === 'error' ? 'error' : 'warn',
    key: i.key,
    args: i.args,
    contentId: i.contentId,
    message: i.message,
  }))
}

async function refreshIssues() {
  const snap = await call('board_snapshot', { boardId: null })
  if (!snap) return
  S.issues = mapIssues(snap.issues)
}

async function refreshSelection() {
  const id = S.selectedId
  if (!id) return
  const sel = await call('record_select', { recordId: id })
  if (S.selectedId !== id) return
  if (sel) await applySelection(sel)
}

export function initEvents() {
  listen('project-changed', () => { refreshProject(); refreshSelection() })
  listen('selection-changed', (payload) => applySelection(payload))
  listen('validation-refreshed', () => refreshIssues())
  listen('game-log', (batch) => { for (const line of batch || []) log(line) })
  listen('import-progress', (p) => {
    if (!p) return
    S.ui.importing = { stage: p.stage, done: p.done, total: p.total }
    if (p.done >= p.total) S.ui.importing = null
  })
  listen('window-close-requested', onCloseRequested)
}


// ---------- 选择 / 视图 ----------

export const selected = () => S.selRecord
export const recordId = (r) => r.id

export function displayName(r) {
  if (!r) return ''
  const key = `${r.type}.${S.project?.name}-${r.name}.name`
  const e = S.project?.strings[key]
  return e?.[S.ui.lang] || r.displayName || r.name
}

export function locSummary(r) {
  const key = `${r.type}.${S.project?.name}-${r.name}.name`
  const e = S.project?.strings[key]
  return { zh: e?.zh || '—', en: e?.en || '—' }
}

export function select(r) {
  S.ui.editRaw = false
  call('record_select', { recordId: r ? r.id : null }).then((payload) => applySelection(payload))
}

export function setType(tp) {
  S.ui.type = tp
  S.ui.page = 0
}

export function setFilter(v) {
  S.ui.filter = v
  S.ui.page = 0
}

export function setView(v) {
  S.ui.view = v
  S.ui.page = 0
}

export function setStringsFilter(v) {
  S.ui.stringsFilter = v
}

// ---------- 缩略图缓存（record_sprite；只取当前页，失败静默回退色块） ----------

export const sprites = reactive(new Map())
const spriteFailed = new Set()
const spritePending = new Set()
const spriteQueue = []
// 本次会话内被导入过贴图的记录：贴图导入已进撤销栈，undo/redo 会改磁盘文件，
// 这些记录的缩略图缓存必须作废重拉（其余记录不受影响，避免全场缩略图闪烁）。
const spriteImported = new Set()
let spriteActive = 0
const SPRITE_CONCURRENCY = 6

async function fetchSprite(id) {
  spriteActive++
  try {
    const res = await invoke('record_sprite', { recordId: id })
    if (res?.dataUrl) sprites.set(id, res.dataUrl)
    else spriteFailed.add(id)
  } catch {
    // 后端接口未就绪或无对应贴图：静默回退，不重试
    spriteFailed.add(id)
  } finally {
    spriteActive--
    spritePending.delete(id)
    pumpSprites()
  }
}

function pumpSprites() {
  while (spriteActive < SPRITE_CONCURRENCY && spriteQueue.length) {
    const id = spriteQueue.shift()
    if (sprites.has(id) || spriteFailed.has(id) || spritePending.has(id)) continue
    spritePending.add(id)
    fetchSprite(id)
  }
}

function requestSprite(r) {
  const id = r?.id
  if (!id || sprites.has(id) || spriteFailed.has(id) || spritePending.has(id)) return
  if (!S.project) return
  spriteQueue.push(id)
  pumpSprites()
}

export function spriteUrl(r) {
  if (!r?.id) return null
  if (!S.ui.showSprites) return null
  const hit = sprites.get(r.id)
  if (hit) return hit
  requestSprite(r)
  return null
}

export function prefetchSprites(list) {
  if (!S.ui.showSprites) return
  for (const r of list || []) requestSprite(r)
}

export function clearSprites() {
  sprites.clear()
  spriteFailed.clear()
  spritePending.clear()
  spriteQueue.length = 0
  spriteImported.clear()
}

/**
 * undo/redo 后作废「导入过贴图」记录的缩略图缓存并重拉。
 * 已知边界：极窄窗口下（作废瞬间仍有在途 fetchSprite）可能把作废前起读的旧值
 * 写回缓存，下一次作废会自愈；不为此引入代际计时。
 */
export function invalidateImportedSprites() {
  if (!spriteImported.size) return
  for (const id of spriteImported) {
    sprites.delete(id)
    spriteFailed.delete(id)
    spritePending.delete(id)
  }
  if (!S.project) return
  for (const r of S.project.records) if (spriteImported.has(r.id)) requestSprite(r)
}

/**
 * 导入贴图：sourcePath 是用户经 PathPicker 选出的源 PNG 绝对路径，目标路径由后端解析。
 * 成功返回与 record_sprite 非空时同形的 DTO，就地写入缩略图缓存即可刷新预览；
 * 后端随后广播的 project-changed 只重拉快照、不清贴图缓存，两条链路互不冲突。
 */
export async function spriteImport(recordId, sourcePath) {
  if (!S.project || !recordId || !sourcePath) return false
  const dto = await call('sprite_import', { recordId, sourcePath })
  if (!dto?.dataUrl) return false
  spriteFailed.delete(recordId)
  sprites.set(recordId, dto.dataUrl)
  spriteImported.add(recordId)
  const name = S.project.records.find((r) => r.id === recordId)?.name || recordId
  log(tr('log.spriteImported', { name }))
  toast(tr('toast.spriteImported', { name }))
  // 导入即触发一次校验重拉，状态栏 badge 与校验页不必等下一次手动校验才一致。
  await refreshIssues()
  return true
}

/**
 * 指定/清除记录的声明类（`record_set_field {field:'type'}`）。
 * `cls` 传 null/空白 = 清除声明，该记录回落到脚本推断或类型默认类。
 * 落盘成功后由后端的 project-changed + selection-changed 广播带出新的有效类与来源，
 * 这里只补一条日志——失败（ok === false）由调用方决定怎么提示。
 */
export async function setClass(r, cls) {
  const name = (cls || '').trim()
  const ok = await call('record_set_field', { recordId: r.id, field: 'type', value: name || null })
  if (ok !== false) log(name ? tr('log.classSet', { cls: name }) : tr('log.classCleared'))
  return ok
}



// ---------- 方块构建画布（NodeCanvasPane 语义；几何在 lib/canvasGeometry.js） ----------

function materializeBoard(snap) {
  if (!snap) return null
  return {
    boards: snap.boards || [],
    board: snap.board,
    nodes: (snap.nodes || []).map((n) => ({ ...n })),
    cards: snap.cards || [],
    edges: (snap.edges || []).map((e) => ({
      ...e,
      reroutes: (e.reroutes || []).map((p) => [p[0], p[1]]),
    })),
    edgeStyle: snap.edgeStyle || 'bezier',
    viewport: snap.viewport || null,
  }
}

export async function refreshBoard(boardIndex = null) {
  if (!S.project) {
    S.board = null
    return
  }
  const snap = await call('board_snapshot', { boardId: boardIndex })
  if (!snap) return
  S.board = materializeBoard(snap)
  S.issues = mapIssues(snap.issues)
}

export function canvasSelectNode(node) {
  if (node?.contentId) {
    const rec = S.project?.records.find((r) => r.id === node.contentId)
    if (rec) {
      select(rec)
      return
    }
  }
  call('record_select', { recordId: null }).then((payload) => applySelection(payload))
}

export function canvasMove(moves) {
  return call('node_move_commit', { moves })
}

export function canvasConnect(fromNode, toNode, fieldKey) {
  return call('edge_connect', { fromNode, toNode, fieldKey })
}

export function canvasDisconnect(edgeId) {
  return call('edge_disconnect', { edgeId })
}

export function canvasInsertReroute(edgeId, x, y) {
  return call('edge_insert_reroute', { edgeId, x, y })
}

export function canvasMoveReroute(edgeId, index, before, after) {
  return call('edge_move_reroute', { edgeId, index, before, after })
}

export function canvasRemoveReroute(edgeId, index) {
  return call('edge_remove_reroute', { edgeId, index })
}

export function canvasDeleteNodes(ids) {
  if (!ids?.length) return null
  return call('record_delete', { recordIds: ids })
}


// ---------- 科技树（对齐 TechTreeRules / TechTreeIndex：research 是唯一数据源） ----------

export const OBJECTIVE_TYPES = ['Research', 'Produce', 'SectorComplete', 'OnSector', 'OnPlanet']

const REQ_KEY = 'requi' + 'rements'

const blankToNull = (v) => (v == null || String(v).trim() === '' ? null : String(v).trim())

export function readResearch(r) {
  if (r && r.research !== undefined) {
    if (!r.research) return null
    return {
      ...r.research,
      reqs: [...(r.research.reqs || [])],
      objectives: (r.research.objectives || []).map((o) => ({ ...o })),
    }
  }
  let v = r?.fields?.research
  if (v == null) return null
  if (typeof v === 'string' && v.trim().startsWith('{')) {
    try { v = JSON.parse(v) } catch { /* 裸父名 */ }
  }
  if (typeof v === 'string') return { parent: blankToNull(v), root: false, name: null, requiresUnlock: false, planet: null, reqs: [], objectives: [] }
  if (typeof v !== 'object' || Array.isArray(v)) return null
  return {
    parent: blankToNull(v.parent),
    root: v.root === true,
    name: blankToNull(v.name),
    requiresUnlock: v.requiresUnlock === true,
    planet: blankToNull(v.planet),
    reqs: Array.isArray(v[REQ_KEY]) ? v[REQ_KEY].map(reqText) : [],
    objectives: Array.isArray(v.objectives) ? v.objectives.filter((o) => o && typeof o === 'object') : [],
  }
}

function reqText(req) {
  if (typeof req === 'string') return req
  if (req && typeof req === 'object' && typeof req.item === 'string') {
    return `${req.item}/${req.amount == null ? 1 : req.amount}`
  }
  return String(req)
}

export function writeResearch(node) {
  if (!node) return undefined
  const bare = !node.root && node.parent && !node.planet && !node.name && !node.requiresUnlock
    && !node.reqs?.length && !node.objectives?.length
  if (bare) return node.parent
  const obj = {}
  if (node.root) {
    obj.root = true
    if (node.name) obj.name = node.name
    if (node.requiresUnlock) obj.requiresUnlock = true
  } else if (node.parent) obj.parent = node.parent
  if (node.planet) obj.planet = node.planet
  if (node.reqs?.length) obj[REQ_KEY] = [...node.reqs]
  if (node.objectives?.length) obj.objectives = node.objectives.map((o) => ({ ...o }))
  return Object.keys(obj).length ? obj : undefined
}

export function objectiveTargetKey(type) {
  if (type === 'SectorComplete' || type === 'OnSector') return 'preset'
  if (type === 'OnPlanet') return 'planet'
  return 'content'
}

const OBJECTIVE_TARGET_KEYS = ['content', 'preset', 'planet']
export function objectiveTarget(obj) {
  for (const k of OBJECTIVE_TARGET_KEYS) if (typeof obj?.[k] === 'string') return obj[k]
  return ''
}

function techRecord(dto) {
  if (!dto) return null
  return {
    id: dto.id,
    type: dto.contentType,
    name: dto.name,
    displayName: dto.displayName,
    patch: dto.patch,
    research: dto.research || null,
    fields: { color: dto.color && dto.color !== '#ffffff' ? dto.color.slice(1) : undefined },
  }
}

function techEntry(dto) {
  const record = techRecord(dto.record)
  const children = (dto.children || []).map(techEntry)
  return {
    record,
    recordId: record ? record.id : null,
    key: dto.key,
    vanilla: !!dto.vanilla,
    problem: dto.problem || null,
    children,
    get label() { return record ? displayName(record) : dto.key },
  }
}

export function techForest() {
  return S.tech
}

export function techCandidates() {
  const names = new Set()
  const walk = (e) => {
    if (e.record && !e.record.patch) names.add(e.record.name)
    for (const c of e.children) walk(c)
  }
  for (const e of S.tech.roots) walk(e)
  for (const list of Object.values(vanillaNames)) for (const n of list) names.add(n)
  return [...names].sort()
}

export function setResearch(r, node) {
  setField(r, 'research', writeResearch(node))
}



// ---------- 星球战役（CosmosPane 语义；与原型同构，写回走壳命令） ----------

export async function refreshCosmosFields() {
  if (!S.project) return
  const ids = S.project.records
    .filter((r) => r.type === 'planet' || r.type === 'sector')
    .map((r) => r.id)
  if (!ids.length) return
  const schemas = await call('records_detail', { recordIds: ids })
  if (!schemas) return
  const byId = new Map(S.project.records.map((r) => [r.id, r]))
  for (const schema of schemas) {
    const r = byId.get(schema.recordId)
    if (r) r.fields = { ...(r.fields || {}), ...typedFields(schema, schema.values) }
  }
}

export async function refreshTechFields() {
  if (!S.project) {
    S.tech = { roots: [], unassigned: [], refused: [] }
    return
  }
  const dto = await call('tech_forest')
  if (!dto) return
  const roots = (dto.roots || []).map(techEntry)
  const unassigned = (dto.unassigned || []).map(techRecord)
  const refused = (dto.refused || []).map(techRecord)
  S.tech = { roots, unassigned, refused }
  const byId = new Map()
  const collect = (e) => {
    if (e.record) byId.set(e.record.id, e.record.research)
    for (const c of e.children) collect(c)
  }
  for (const e of roots) collect(e)
  for (const r of unassigned) byId.set(r.id, null)
  for (const r of refused) byId.set(r.id, r.research)
  for (const r of S.project.records) if (byId.has(r.id)) r.research = byId.get(r.id)
}

export function setCosmosTab(i) {
  S.ui.cosmosTab = i
}
export function setCosmosSearch(v) {
  S.ui.cosmosSearch = v
}


// ---------- 星球场景（cosmos_scene；后端未就绪时静默返回 null，保留空态） ----------

export const cosmosScenes = reactive(new Map())
const cosmosPending = new Map()
const cosmosFailed = new Set()
let cosmosGen = 0

export function clearCosmosScenes() {
  cosmosGen++
  cosmosScenes.clear()
  cosmosPending.clear()
  cosmosFailed.clear()
}

/** 按 recordId 取场景，同 id 并发请求合并；失败缓存为 null，避免反复打未实现的命令。 */
export async function cosmosScene(recordId) {
  if (!recordId) return null
  if (cosmosScenes.has(recordId)) return cosmosScenes.get(recordId)
  if (cosmosPending.has(recordId)) return cosmosPending.get(recordId)
  const gen = cosmosGen
  const task = (async () => {
    try {
      const dto = await invoke('cosmos_scene', { recordId })
      if (gen === cosmosGen) cosmosScenes.set(recordId, dto || null)
      return dto || null
    } catch {
      if (gen === cosmosGen) {
        cosmosFailed.add(recordId)
        cosmosScenes.set(recordId, null)
      }
      return null
    } finally {
      // 清缓存后可能已发起同 id 的新请求，别误删新的 pending
      if (cosmosPending.get(recordId) === task) cosmosPending.delete(recordId)
    }
  })()
  cosmosPending.set(recordId, task)
  return task
}

export const sectorPlanet = (r) =>
  typeof r.fields.planet === 'string' && r.fields.planet.trim() ? r.fields.planet : 'serpulo'

const barePlanet = (r) => {
  const mod = S.project?.internalName || S.project?.name
  const p = sectorPlanet(r)
  return mod && p.startsWith(mod + '-') ? p.slice(mod.length + 1) : p
}

export function cosmosGroups() {
  if (!S.project) return []
  const byPlanet = new Map()
  for (const r of S.project.records) {
    if (r.type !== 'sector') continue
    const key = barePlanet(r)
    if (!byPlanet.has(key)) byPlanet.set(key, [])
    byPlanet.get(key).push(r)
  }
  const groups = []
  for (const r of S.project.records) {
    if (r.type !== 'planet') continue
    groups.push({ key: r.name, planet: r, vanilla: false, sectors: byPlanet.get(r.name) || [] })
    byPlanet.delete(r.name)
  }
  for (const [key, sectors] of byPlanet) groups.push({ key, planet: null, vanilla: true, sectors })
  return groups
}

export function cosmosTree() {
  const groups = cosmosGroups()
  const q = S.ui.cosmosSearch.trim().toLowerCase()
  if (!q) return groups
  return groups
    .map((g) => {
      const groupLabel = (g.planet ? displayName(g.planet) : g.key).toLowerCase()
      if (groupLabel.includes(q)) return g
      return { ...g, sectors: g.sectors.filter((s) => displayName(s).toLowerCase().includes(q) || s.name.toLowerCase().includes(q)) }
    })
    .filter((g) => g.sectors.length > 0 || (g.planet && displayName(g.planet).toLowerCase().includes(q)) || g.key.toLowerCase().includes(q))
}

export function cosmosSelection() {
  const s = selected()
  if (s && (s.type === 'planet' || s.type === 'sector')) return s
  if (!S.project) return null
  const planet = S.project.records.find((r) => r.type === 'planet' && (Number(r.fields.sectorSize) || 0) > 0)
  return planet || S.project.records.find((r) => r.type === 'planet') || S.project.records.find((r) => r.type === 'sector') || null
}

export function selectDefaultCosmos() {
  const s = selected()
  if (s && (s.type === 'planet' || s.type === 'sector')) return
  const fallback = cosmosSelection()
  if (fallback) select(fallback)
}

export function planetGrid(rec) {
  const target = rec && rec.type === 'planet'
    ? rec
    : S.project?.records.find((r) => r.type === 'planet' && r.name === (rec ? barePlanet(rec) : ''))
  const size = Number(target?.fields?.sectorSize) || 0
  const capacity = size * size
  const sectors = S.project
    ? S.project.records.filter((r) => r.type === 'sector' && target && barePlanet(r) === target.name)
    : []
  const byIndex = new Map()
  for (const s of sectors) {
    const n = Number(s.fields.sector)
    if (Number.isFinite(n)) byIndex.set(n, s)
  }
  return { planet: target || null, size, capacity, occupied: sectors.length, byIndex, sectors }
}

export function planetColors(rec) {
  const p = rec && rec.type === 'planet' ? rec : planetGrid(rec).planet
  let mesh = p?.fields?.mesh
  if (typeof mesh === 'string') {
    try { mesh = JSON.parse(mesh) } catch { mesh = null }
  }
  const hex = (v, fallback) => (typeof v === 'string' ? '#' + v.replace(/^#/, '').slice(0, 6) : fallback)
  return {
    c1: hex(mesh?.color1, '#7FB77E'),
    c2: hex(mesh?.color2, '#DFF0D8'),
    meshType: mesh && typeof mesh.type === 'string' ? mesh.type : '—',
  }
}

export async function reassignSector(sector, planetKey) {
  if (!sector || !planetKey) return false
  const current = sectorPlanet(sector)
  if (current === planetKey || barePlanet(sector) === planetKey) return false
  await setField(sector, 'planet', planetKey)
  return true
}

export async function addCosmos(type) {
  const before = selected()
  const payload = await call('record_add', { contentType: type })
  if (!payload) return
  await applySelection(payload)
  if (type === 'sector' && before && before.type === 'planet') {
    await setField({ id: payload.id }, 'planet', before.name)
  }
  refreshProject()
}

export function importCancel() {
  call('import_cancel')
  S.ui.importing = null
}

export function typeRecords() {
  if (!S.project) return []
  const f = S.ui.filter.trim().toLowerCase()
  return S.project.records
    .filter((r) => r.type === S.ui.type)
    .filter((r) => !f || r.name.toLowerCase().includes(f) || displayName(r).toLowerCase().includes(f))
    .sort((a, b) => a.name.localeCompare(b.name))
}

export const PER_PAGE = 35

export function pageCount() {
  return Math.max(1, Math.ceil(typeRecords().length / PER_PAGE))
}

export function pageRecords() {
  const start = S.ui.page * PER_PAGE
  return typeRecords().slice(start, start + PER_PAGE)
}

export function issuesFor(r) {
  return S.issues.filter((i) => i.contentId === r.id)
}

export const errorCount = () => S.issues.filter((i) => i.level === 'error').length
export const warnCount = () => S.issues.filter((i) => i.level === 'warn').length
export const canUndo = () => S.undoState.canUndo
export const canRedo = () => S.undoState.canRedo


// ---------- 命令 ----------

export async function undo() {
  const h = await call('undo')
  applyHistory(h)
}

export async function redo() {
  const h = await call('redo')
  applyHistory(h)
}

export async function setField(r, field, value) {
  const text = value === undefined || value === null ? null : typeof value === 'object' ? JSON.stringify(value) : String(value)
  const ok = await call('record_set_field', { recordId: r.id, field, value: text })
  if (ok === false) {
    log(tr('log.fieldBad', { v: text }))
    toast(tr('log.fieldBad', { v: text }), 'warn')
  }
  // 星球/区块字段会改变场景（网格、外观、轨道），场景缓存必须失效并通知 Cosmos 预览重拉。
  if (ok !== false && (!r.type || r.type === 'planet' || r.type === 'sector')) {
    clearCosmosScenes()
    S.ui.cosmosRev++
  }
  return ok
}

export async function setPatch(r, value) {
  await call('record_set_patch', { recordId: r.id, patch: value })
}

export function renameRecord(r, newName) {
  newName = newName.trim()
  if (newName === r.name) return null
  if (!NAME_RE.test(newName)) return tr('dlg.renameBad')
  if (S.project.records.some((x) => x !== r && x.type === r.type && x.name === newName)) return tr('dlg.renameDup')
  call('record_rename', { recordId: r.id, name: newName }).then(() => {
    log(tr('log.renamed', { old: r.name, new: newName }))
  })
  return null
}

export async function addRecord(type = S.ui.type) {
  if (!S.project) return
  const payload = await call('record_add', { contentType: type })
  if (payload) {
    log(tr('log.added', { type: payload.contentType, name: payload.name }))
    await applySelection(payload)
    refreshProject()
  }
}

/** 删除指定记录（右键菜单用：不必先切换全局选择）。 */
export async function deleteRecord(r) {
  if (!r) return
  log(tr('log.deleted', { type: r.type, name: r.name }))
  // 2026-11-26 修复：删选中项前先清掉前端选择。否则 project-changed 刷新链
  // （refreshSelection / 检查器 record_schema）会拿已删 id 去查，弹「找不到记录」错误 toast。
  if (S.selectedId && r.id === S.selectedId) await applySelection(null)
  await call('record_delete', { recordIds: [r.id] })
}

export async function deleteSelected(type = S.ui.type) {
  const r = selected()
  if (!r || (type && r.type !== type)) return
  await deleteRecord(r)
}

export async function setString(key, lang, value) {
  await call('string_set', { key, locale: localeId(lang), value })
  if (S.project?.strings[key]) S.project.strings[key][lang] = value
}

export async function applyRaw(r, text) {
  const ok = await call('record_apply_raw', { recordId: r.id, text })
  if (ok) {
    log(tr('log.rawApplied'))
    await refreshSelection()
    return true
  }
  log(tr('log.rawBad', { e: 'parse failed' }))
  return false
}


// ---------- 校验 / 搜索 / 构建 / 游戏 ----------

export async function validateNow(switchTab = true) {
  if (!S.project) return
  S.ui.validating = true
  log(tr('log.validating'))
  await refreshIssues()
  S.ui.validating = false
  log(tr('log.findings', { n: S.issues.length }))
  for (const i of S.issues.slice(0, 10)) {
    log(`  ${i.level === 'error' ? tr('issues.error') : tr('issues.warn')} ${i.message}`)
  }
  if (switchTab) S.ui.inspectorTab = 'validate'
}

export function searchNext() {
  if (!S.project) return
  const q = S.ui.search.trim().toLowerCase()
  if (!q) return
  const matches = S.project.records.filter(
    (r) => r.name.toLowerCase().includes(q) || displayName(r).toLowerCase().includes(q),
  )
  if (!matches.length) {
    log(tr('log.noMatch', { q: S.ui.search }))
    S.searchIdx = -1
    return
  }
  S.searchIdx = (S.searchIdx + 1) % matches.length
  const r = matches[S.searchIdx]
  S.ui.workspace = 'assets'
  setType(r.type)
  const list = typeRecords()
  const pos = list.findIndex((x) => x.id === r.id)
  if (pos >= 0) S.ui.page = Math.floor(pos / PER_PAGE)
  select(r)
  log(`[${S.searchIdx + 1}/${matches.length}] ${r.type}/${r.name}`)
}

/**
 * 构建到指定目录。destination/zip 由 PathPicker 收集（不再 window.prompt）。
 * @returns {Promise<boolean>} 是否已写入
 */
export async function buildProject(destination, zip = false) {
  if (!S.project) return false
  const plan = await call('project_plan_build')
  if (!plan) return false
  if (plan.hasErrors && !window.confirm(tr('dlg.buildErrors', { n: plan.errorCount }))) return false
  const report = await call('project_write_build', { destination, zip: !!zip })
  if (report) {
    log(report.summary)
    S.ui.modal = { kind: 'buildDone', dir: destination }
    return true
  }
  return false
}

export async function installAndLaunch() {
  if (!S.project) return
  const session = await call('game_launch')
  if (session) {
    S.ui.session = { dir: session.dataDir }
    S.ui.gameRunning = session.running
    log(tr('log.launched', { dir: session.dataDir }))
  }
}

export async function stopGame() {
  const session = await call('game_stop')
  if (session) {
    S.ui.gameRunning = false
    log(tr('log.stopped'))
  }
}

export function openSessionDir() {
  if (!S.ui.session) return
  log(tr('log.openSession', { dir: S.ui.session.dir }))
}


// ---------- 项目生命周期 ----------

export async function save(kind = 'save') {
  if (!S.project) return
  const summary = await call('project_save')
  if (summary) {
    materializeSummary(summary)
    S.savedAt = new Date().toTimeString().slice(0, 5)
    S.savedKind = kind
    log(kind === 'auto' ? tr('log.autoSaved') : tr('log.saved', { name: S.project.name }))
  }
}

export function guardDiscard() {
  return !S.dirty || window.confirm(tr('dlg.discard'))
}


// ---------- 原生关窗守卫（P1-4 前端接线） ----------
// 壳契约：原生 X / Alt+F4 / 应用内 close() 都被壳拦下并广播 window-close-requested
// （空载荷，3s 无人应答兜底放行）。收到事件立即调 window_close_hold 摘除该代兜底计时，
// 再按 S.dirty（history_state 同源口径）分支：干净 → window_confirm_close；
// dirty → closeConfirm 对话框（保存并关闭 / 放弃并关闭 / 取消）；取消不调任何命令。
// not-found（重复 hold / 兜底已放行 / 窗口已 destroy）按契约静默忽略。

async function closeGuardCall(cmd) {
  try {
    await invoke(cmd)
  } catch (e) {
    if (errCode(e) === 'not-found') return
    log(`[${errCode(e)}] ${errMsg(e)}`)
    toastCommandError(e, cmd)
  }
}

async function onCloseRequested() {
  // hold 先于任何 dirty 判定与弹窗：对话框由此获得无限决策时间。
  await closeGuardCall('window_close_hold')
  if (!S.dirty) {
    S.ui.modal = null
    await closeGuardCall('window_confirm_close')
    return
  }
  // 连按 X：壳发新代、旧代作废，上面的 hold 已摘除新代计时；
  // 对话框已开时保持同一实例（去重不叠加，也不打断进行中的保存）。
  if (S.ui.modal?.kind !== 'closeConfirm') S.ui.modal = { kind: 'closeConfirm', busy: false }
}

/** 保存并关闭：project_save 成功（S.dirty 清零）才 confirm；失败保留对话框（call 已提示）。 */
export async function closeConfirmSave() {
  const m = S.ui.modal
  if (m?.kind !== 'closeConfirm' || m.busy) return
  m.busy = true
  await save('save')
  m.busy = false
  // 保存期间对话框被取消 → 不替用户关窗；保存失败（仍 dirty）→ 留在对话框。
  if (S.dirty || S.ui.modal?.kind !== 'closeConfirm') return
  S.ui.modal = null
  await closeGuardCall('window_confirm_close')
}

/** 放弃并关闭：不保存，直接 confirm。 */
export async function closeConfirmDiscard() {
  S.ui.modal = null
  await closeGuardCall('window_confirm_close')
}

/** 取消：hold 已摘除兜底计时，不调任何命令，窗口保持打开。 */
export function closeConfirmCancel() {
  S.ui.modal = null
}

function afterProjectSwitch() {
  S.savedAt = null
  S.searchIdx = -1
  S.ui.workspace = 'assets'
  S.ui.type = 'item'
  S.ui.page = 0
  S.ui.filter = ''
  S.selectedId = null
  S.selRecord = null
  S.selSchema = []
  S.selClassName = null
  S.selClassSource = 'declared'
  S.ui.rawDraft = ''
  S.ui.importing = null
  S.tech = { roots: [], unassigned: [], refused: [] }
  S.board = null
  clearSprites()
  clearCosmosScenes()
}

/** 新建项目：parent 为父目录，name 为项目名（root = parent/name）。 */
export async function newProject(name, root) {
  if (!name || !root) return false
  if (!NAME_RE.test(name)) {
    log(tr('dlg.renameBad'))
    return false
  }
  const summary = await call('project_create', { name, root })
  if (!summary) return false
  afterProjectSwitch()
  materializeSummary(summary)
  await refreshProject()
  log(tr('log.newProject', { name }))
  refreshIssues()
  return true
}

/** 打开项目：path 必须是含 mod.json / mod.hjson 的目录。 */
export async function openProject(path) {
  if (!path) return false
  const summary = await call('project_open', { path })
  if (!summary) return false
  afterProjectSwitch()
  materializeSummary(summary)
  await refreshProject()
  log(tr('log.opened', { path }))
  refreshIssues()
  return true
}

/** 导入 mod（目录或 .zip/.jar），写入 destination。 */
export async function importMod(source, destination) {
  if (!source || !destination) return false
  const report = await call('project_import', { source, destination })
  S.ui.importing = null
  if (!report) return false
  log(report.summary)
  if (report.warnings?.length) for (const w of report.warnings) log(`! ${w}`)
  afterProjectSwitch()
  await refreshProject()
  refreshIssues()
  toast(tr('toast.imported', { n: report.contentCount }))
  return true
}

export async function openDemo() {
  if (!guardDiscard()) return
  const summary = await call('project_open_demo')
  if (summary) {
    materializeSummary(summary)
    S.ui.type = 'item'
    S.ui.page = 0
    S.ui.filter = ''
    S.ui.workspace = 'assets'
    S.ui.inspectorTab = 'props'
    S.savedAt = null
    S.searchIdx = -1
    clearSprites()
    clearCosmosScenes()
    await refreshProject()
    const copper = S.project.records.find((r) => r.name === 'copper' && r.type === 'item')
    if (copper) select(copper)
    log(tr('log.imported', { name: S.project.name, n: S.project.records.length }))
    refreshIssues()
  }
}


// ---------- 视图 / 格式（后端新 IPC，未就绪时静默降级并提示） ----------

export function toggleSprites() {
  S.ui.showSprites = !S.ui.showSprites
  log(tr('log.sprites', { v: tr(S.ui.showSprites ? 'log.on' : 'log.off') }))
}

export function requestFit() {
  S.ui.fitTick++
}

/** 整理方块画布布局；返回是否成功。 */
export async function tidyBoard() {
  if (!S.project) return false
  const snap = await call('board_tidy_layout')
  if (!snap) return false
  S.board = materializeBoard(snap)
  S.issues = mapIssues(snap.issues)
  log(tr('log.tidied'))
  return true
}

/** 重建方块连线；返回改动边数，null 表示接口不可用。 */
export async function rebuildLinks() {
  if (!S.project) return null
  const n = await call('project_rebuild_links')
  if (n == null) return null
  log(tr('log.relinked', { n }))
  await refreshBoard()
  return n
}

/** 项目格式转换 json | hjson；返回转换文件数，null 表示接口不可用。 */
export async function convertFormat(ext) {
  if (!S.project) return null
  const n = await call('project_convert_format', { ext })
  if (n == null) return null
  log(n ? tr('log.converted', { n }) : tr('log.nothingToConvert'))
  return n
}


// ---------- 工作区通用查询（units 复用，不污染 S.ui.type） ----------

export function recordsOfType(type) {
  if (!S.project) return []
  return S.project.records
    .filter((r) => r.type === type)
    .sort((a, b) => a.name.localeCompare(b.name))
}

export function matchesQuery(list, q) {
  const f = (q || '').trim().toLowerCase()
  if (!f) return list
  return list.filter((r) => r.name.toLowerCase().includes(f) || displayName(r).toLowerCase().includes(f))
}

export { isVanilla, TYPES }
