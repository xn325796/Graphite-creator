// ============================================================
// 动作层 + 命令表（单一来源）
// 菜单、命令面板、快捷键帮助、全局快捷键都从这里读取，
// 避免同一个动作在多处各写一份。
// ============================================================
import { nextTick, reactive } from 'vue'
import {
  S, tr, log, canUndo, canRedo, undo, redo, validateNow, searchNext,
  buildProject, installAndLaunch, stopGame, openSessionDir, save,
  newProject, openProject, importMod, settleSelection, guardDiscard,
  tidyBoard, rebuildLinks, convertFormat, requestFit, toggleSprites,
  invalidateImportedSprites,
} from '../store/project.js'
import { toast } from './toast.js'
import { minimizeWindow, toggleMaximizeWindow, closeWindow, windowState, refreshWindowState } from './window.js'
import { clearDrafts } from '../shared/fieldDrafts.js'
import { resetAllPanelWidths } from './panelWidth.js'
import { themeState, setTheme } from './theme.js'

// ---------- 覆盖层状态（由 Home 渲染） ----------

export const paletteState = reactive({ open: false })
export const shortcutsOpen = reactive({ open: false })

export function openPalette() { paletteState.open = true }
export function closePalette() { paletteState.open = false }
export function openShortcuts() { shortcutsOpen.open = true }
export function closeShortcuts() { shortcutsOpen.open = false }

/** 把键盘焦点送到检查器的名称输入框（右键菜单「重命名」）。 */
export function focusInspectorName() {
  if (!S.project || !S.selRecord) return
  if (S.ui.workspace !== 'assets' && S.ui.workspace !== 'units') S.ui.workspace = 'assets'
  S.ui.inspectorTab = 'props'
  nextTick(() => {
    const el = document.querySelector('[data-inspector-name] input')
    if (!el) return
    el.focus()
    el.select?.()
  })
}

// ---------- 路径选择器 ----------

export const picker = reactive({
  open: false,
  step: '',
  mode: 'dir',
  title: '',
  filter: [],
  initial: '',
  path: '',
  busy: false,
  newName: 'new-mod',
  buildZip: false,
  importSource: '',
})

const NAME_RE = /^[a-z0-9]+(-[a-z0-9]+)*$/
const parentDir = (p) => {
  const s = String(p || '').replace(/\\/g, '/').replace(/\/+$/, '')
  const i = s.lastIndexOf('/')
  return i > 0 ? s.slice(0, i) : s
}
const joinPath = (a, b) => {
  const s = String(a || '').replace(/\\/g, '/').replace(/\/+$/, '')
  return s.endsWith('/') ? s + b : s + '/' + b
}
const defaultDir = () => {
  const p = S.project?.path
  if (p && p !== '(未保存)' && /[\\/]/.test(p)) return parentDir(p)
  return 'C:/mods'
}
const defaultBuildDir = () => {
  const p = S.project?.path
  if (p && p !== '(未保存)' && /[\\/]/.test(p)) return joinPath(parentDir(p), `${S.project.name}-build`)
  return 'C:/mods/build'
}

export const rootPreview = () => (NAME_RE.test(picker.newName) ? joinPath(picker.path, picker.newName) : '')
export const pickerConfirmDisabled = () =>
  picker.busy || (picker.step === 'new-parent' && !rootPreview())

function openPicker({ step, mode = 'dir', title, filter = [], initial = '' }) {
  picker.step = step
  picker.mode = mode
  picker.title = title
  picker.filter = filter
  picker.initial = initial
  picker.path = initial
  picker.open = true
}
export function closePicker() {
  picker.open = false
  picker.step = ''
  picker.busy = false
}

export function startNewProject() {
  if (!guardDiscard()) return
  picker.newName = 'new-mod'
  openPicker({ step: 'new-parent', title: tr('picker.title.parent'), initial: defaultDir() })
}
export function startOpenProject() {
  if (!guardDiscard()) return
  openPicker({ step: 'open', title: tr('picker.title.open'), initial: defaultDir() })
}
export function startImportDir() {
  if (!guardDiscard()) return
  openPicker({ step: 'import-dir-source', title: tr('picker.title.importSource'), initial: defaultDir() })
}
export function startImportZip() {
  if (!guardDiscard()) return
  openPicker({
    step: 'import-zip-source',
    mode: 'file',
    filter: ['.zip', '.jar'],
    title: tr('picker.title.importArchive'),
    initial: defaultDir(),
  })
}
export function startBuild() {
  if (!S.project) { toast(tr('toast.needProject'), 'warn'); return }
  picker.buildZip = false
  openPicker({ step: 'build', title: tr('picker.title.buildDest'), initial: defaultBuildDir() })
}

export async function onPickerConfirm({ path }) {
  const step = picker.step
  picker.busy = true
  try {
    if (step === 'new-parent') {
      if (!rootPreview()) { toast(tr('picker.nameBad'), 'warn'); return }
      if (await newProject(picker.newName, rootPreview())) closePicker()
    } else if (step === 'open') {
      if (await openProject(path)) closePicker()
    } else if (step === 'import-dir-source' || step === 'import-zip-source') {
      picker.importSource = path
      const base = step === 'import-zip-source' ? path.replace(/\.[^./]+$/, '') : path
      openPicker({ step: 'import-dest', title: tr('picker.title.importDest'), initial: base + '-import' })
    } else if (step === 'import-dest') {
      if (await importMod(picker.importSource, path)) closePicker()
    } else if (step === 'build') {
      if (await buildProject(path, picker.buildZip)) closePicker()
    }
  } finally {
    picker.busy = false
  }
}

// ---------- 常用动作 ----------

export function focusSearch() {
  const input = document.querySelector('[data-global-search] input')
  if (input) { input.focus(); input.select?.() }
}

// undo/redo 可能改动了磁盘上的贴图文件（贴图导入已入撤销栈），
// 导入过的记录缩略图缓存必须作废重拉，否则界面会停留在旧图。
export async function doUndo() { await undo(); await settleSelection(); clearDrafts(); invalidateImportedSprites() }
export async function doRedo() { await redo(); await settleSelection(); clearDrafts(); invalidateImportedSprites() }
export async function doSave() {
  await save('save')
  if (S.project) toast(tr('toast.saved', { name: S.project.name }))
}
export async function doValidate() {
  await validateNow()
  toast(tr('toast.validated', { n: S.issues.length }), errorCountSafe() ? 'warn' : 'info')
}
const errorCountSafe = () => S.issues.filter((i) => i.level === 'error').length

export const canFit = () => !!S.project && (S.ui.workspace === 'canvas' || S.ui.workspace === 'tech')

export async function arrangeLayout() {
  if (!S.project) return
  if (S.ui.workspace !== 'canvas') S.ui.workspace = 'canvas'
  if (!(await tidyBoard())) toast(tr('toast.unsupported', { cmd: 'board_tidy_layout' }), 'warn')
}
export async function relink() {
  if (!S.project) return
  const n = await rebuildLinks()
  if (n == null) toast(tr('toast.unsupported', { cmd: 'project_rebuild_links' }), 'warn')
  else toast(tr('toast.relinked', { n }))
}
export async function fitView() {
  if (!canFit()) { toast(tr('hint.viewOnly'), 'warn'); return }
  requestFit()
  log(tr('log.fit'))
}
export async function doConvert(ext) {
  if (!S.project) return
  const n = await convertFormat(ext)
  if (n == null) toast(tr('toast.unsupported', { cmd: 'project_convert_format' }), 'warn')
  else if (n === 0) toast(tr('toast.noConvert'))
  else toast(tr('toast.converted', { n }))
}
export function switchLang(l) {
  S.ui.lang = l
  log(tr('log.langSwitch'))
}
export function requestClose() {
  // 与原生 X 同路径：close() 触发壳的 CloseRequested 拦截 → window-close-requested，
  // hold / dirty 判定 / 关闭确认对话框统一由 store 的关窗守卫处理。
  closeWindow()
}

// ---------- 命令定义 ----------

const needProject = () => (S.project ? '' : tr('hint.needProject'))

const WORKSPACES = [
  { id: 'assets', shortcut: 'Ctrl+1' },
  { id: 'canvas', shortcut: 'Ctrl+2' },
  { id: 'units', shortcut: 'Ctrl+3' },
  { id: 'cosmos', shortcut: 'Ctrl+4' },
  { id: 'tech', shortcut: 'Ctrl+5' },
]

export const commands = [
  // 项目
  { id: 'project.new', menu: 'project', title: () => tr('project.new'), shortcut: 'Ctrl+N', run: startNewProject },
  { id: 'project.open', menu: 'project', title: () => tr('project.open'), shortcut: 'Ctrl+O', run: startOpenProject },
  { id: 'project.importZip', menu: 'project', title: () => tr('project.importZip'), run: startImportZip },
  { id: 'project.importDir', menu: 'project', title: () => tr('project.importDir'), run: startImportDir },
  { id: 'project.save', menu: 'project', title: () => tr('project.save'), shortcut: 'Ctrl+S', enabled: () => !!S.project, run: doSave },
  { id: 'project.meta', menu: 'project', title: () => tr('project.meta'), enabled: () => !!S.project, hint: needProject, run: () => { S.ui.modal = { kind: 'meta' } } },
  { id: 'project.find', menu: 'project', title: () => tr('project.find'), shortcut: 'Ctrl+F', run: focusSearch },
  { id: 'project.exit', menu: 'project', title: () => tr('project.exit'), run: requestClose },

  // 编辑
  { id: 'edit.undo', menu: 'edit', title: () => tr('edit.undo'), shortcut: 'Ctrl+Z', enabled: canUndo, run: doUndo },
  { id: 'edit.redo', menu: 'edit', title: () => tr('edit.redo'), shortcut: 'Ctrl+Y', enabled: canRedo, run: doRedo },

  // 视图
  ...WORKSPACES.map(({ id, shortcut }) => ({
    id: `view.workspace.${id}`,
    menu: 'view',
    title: () => tr(`ws.${id}`),
    shortcut,
    checked: () => S.ui.workspace === id,
    run: () => { S.ui.workspace = id },
  })),
  { id: 'view.layout', menu: 'view', title: () => tr('view.layout'), enabled: () => !!S.project, hint: needProject, run: arrangeLayout },
  { id: 'view.relink', menu: 'view', title: () => tr('view.relink'), enabled: () => !!S.project, hint: needProject, run: relink },
  { id: 'view.fit', menu: 'view', title: () => tr('view.fit'), enabled: canFit, hint: () => tr('hint.viewOnly'), run: fitView },
  { id: 'view.sprites', menu: 'view', title: () => tr('view.sprites'), checked: () => S.ui.showSprites, run: toggleSprites },
  { id: 'view.log', menu: 'view', title: () => tr('view.log'), checked: () => S.ui.logOpen, run: () => { S.ui.logOpen = !S.ui.logOpen } },
  { id: 'view.resetPanels', menu: 'view', title: () => tr('view.resetPanels'), run: () => { resetAllPanelWidths(); toast(tr('toast.panelsReset')) } },
  { id: 'view.palette', menu: 'view', title: () => tr('view.palette'), shortcut: 'Ctrl+K', run: openPalette },
  { id: 'view.minimalUI', menu: 'view', title: () => tr('view.minimalUI'), shortcut: 'Ctrl+\\', checked: () => S.ui.minimal, run: () => { S.ui.minimal = !S.ui.minimal } },

  // 主题
  { id: 'view.themeDark', menu: 'view', title: () => tr('view.themeDark'), checked: () => themeState.mode === 'dark', run: () => setTheme('dark') },
  { id: 'view.themeLight', menu: 'view', title: () => tr('view.themeLight'), checked: () => themeState.mode === 'light', run: () => setTheme('light') },
  { id: 'view.themeSystem', menu: 'view', title: () => tr('view.themeSystem'), checked: () => themeState.mode === 'system', run: () => setTheme('system') },

  // 运行
  { id: 'run.validate', menu: 'run', title: () => tr('cmd.validate'), shortcut: 'F5', run: doValidate },
  { id: 'run.build', menu: 'run', title: () => tr('cmd.build'), shortcut: 'Ctrl+B', run: startBuild },
  { id: 'run.launch', menu: 'run', title: () => tr('cmd.launch'), shortcut: 'F6', run: installAndLaunch },
  { id: 'run.stop', menu: 'run', title: () => tr('cmd.stop'), enabled: () => !!S.ui.gameRunning, run: stopGame },
  { id: 'run.openDir', menu: 'run', title: () => tr('cmd.openDir'), enabled: () => false, hint: () => tr('hint.openDir'), run: openSessionDir },

  // 格式
  { id: 'format.toJson', menu: 'format', title: () => tr('format.toJson'), enabled: () => !!S.project, hint: needProject, run: () => doConvert('json') },
  { id: 'format.toHjson', menu: 'format', title: () => tr('format.toHjson'), enabled: () => !!S.project, hint: needProject, run: () => doConvert('hjson') },

  // 语言
  { id: 'language.zh', menu: 'language', title: () => '简体中文', checked: () => S.ui.lang === 'zh', run: () => switchLang('zh') },
  { id: 'language.en', menu: 'language', title: () => 'English', checked: () => S.ui.lang === 'en', run: () => switchLang('en') },

  // 窗口
  { id: 'window.min', menu: 'window', title: () => tr('window.min'), run: minimizeWindow },
  { id: 'window.max', menu: 'window', title: () => tr(windowState.maximized ? 'window.restore' : 'window.max'), run: async () => { await toggleMaximizeWindow(); await refreshWindowState() } },
  { id: 'window.close', menu: 'window', title: () => tr('window.close'), run: requestClose },

  // 帮助
  { id: 'help.shortcuts', menu: 'help', title: () => tr('help.keys'), shortcut: 'F1', run: openShortcuts },
  { id: 'help.about', menu: 'help', title: () => tr('help.about'), run: () => { S.ui.modal = { kind: 'about' } } },
]

export const commandById = (id) => commands.find((c) => c.id === id)

export function isCommandEnabled(cmd) {
  return cmd.enabled ? !!cmd.enabled() : true
}

export function runCommand(cmd) {
  if (!cmd || !isCommandEnabled(cmd)) {
    if (cmd?.hint) toast(typeof cmd.hint === 'function' ? cmd.hint() : cmd.hint, 'warn')
    return false
  }
  cmd.run?.()
  return true
}

// 菜单结构：id 指向命令；'|' 为分隔线
export const MENU_LAYOUT = [
  {
    id: 'project',
    label: () => tr('menu.project'),
    items: ['project.new', 'project.open', 'project.importZip', 'project.importDir', '|', 'project.save', 'project.meta', 'project.find', '|', 'project.exit'],
  },
  { id: 'edit', label: () => tr('menu.edit'), items: ['edit.undo', 'edit.redo'] },
  {
    id: 'view',
    label: () => tr('menu.view'),
    items: ['view.workspace.assets', 'view.workspace.canvas', 'view.workspace.units', 'view.workspace.cosmos', 'view.workspace.tech', '|', 'view.layout', 'view.relink', 'view.fit', 'view.sprites', 'view.log', 'view.resetPanels', '|', 'view.palette', 'view.minimalUI', '|', 'view.themeDark', 'view.themeLight', 'view.themeSystem'],
  },
  { id: 'run', label: () => tr('menu.run'), items: ['run.validate', 'run.build', 'run.launch', 'run.stop', '|', 'run.openDir'] },
  { id: 'format', label: () => tr('menu.format'), items: ['format.toJson', 'format.toHjson'] },
  { id: 'language', label: () => tr('menu.language'), items: ['language.zh', 'language.en'] },
  { id: 'window', label: () => tr('menu.window'), items: ['window.min', 'window.max', 'window.close'] },
  { id: 'help', label: () => tr('menu.help'), items: ['help.shortcuts', 'help.about'] },
]

export function menuItems(menu) {
  return menu.items.map((id) => {
    if (id === '|') return { sep: true }
    const cmd = commandById(id)
    return {
      label: cmd.title(),
      shortcut: cmd.shortcut,
      checked: cmd.checked ? !!cmd.checked() : false,
      disabled: !isCommandEnabled(cmd),
      hint: cmd.hint ? (typeof cmd.hint === 'function' ? cmd.hint() : cmd.hint) : '',
      action: () => runCommand(cmd),
    }
  })
}

// 快捷键帮助：按菜单分组（仅列出有快捷键的命令），随语言实时计算
export function shortcutGroups() {
  return MENU_LAYOUT
    .map((m) => ({
      label: m.label(),
      items: m.items
        .filter((id) => id !== '|')
        .map((id) => commandById(id))
        .filter((c) => c && c.shortcut)
        .map((c) => ({ label: c.title(), keys: c.shortcut })),
    }))
    .filter((g) => g.items.length)
}

export const GESTURES = [
  { keys: 'Wheel', label: () => tr('gesture.wheel') },
  { keys: 'Space + Drag', label: () => tr('gesture.pan') },
  { keys: 'Delete', label: () => tr('gesture.delete') },
  { keys: 'Shift + Click', label: () => tr('gesture.multi') },
  { keys: 'Double Click', label: () => tr('gesture.double') },
]
