<script setup>
import { computed, ref, watch, onMounted, onUnmounted } from 'vue'
import { S, tr, canUndo, canRedo, log, searchNext } from '../../store/project.js'
import {
  MENU_LAYOUT, menuItems, commandById, paletteState,
  picker, pickerConfirmDisabled, closePicker, onPickerConfirm, rootPreview,
  startBuild, doValidate, doUndo, doRedo, requestClose,
} from '../../lib/actions.js'
import {
  minimizeWindow, toggleMaximizeWindow, onWindowResized, refreshWindowState, windowState,
} from '../../lib/window.js'
import { modalState } from '../../lib/modalState.js'
import ArButton from '../ui/ArButton.vue'
import ArInput from '../ui/ArInput.vue'
import ArSwitch from '../ui/ArSwitch.vue'
import ArDropdown from '../ui/ArDropdown.vue'
import ArTabs from '../ui/ArTabs.vue'
import ArTooltip from '../ui/ArTooltip.vue'
import PathPicker from './PathPicker.vue'

const activeMenu = ref(null)
const menuButtons = ref([])

// 命令面板或对话框打开时收起菜单，避免浮层叠压
watch([() => paletteState.open, () => modalState.open], ([palette, modals]) => {
  if (palette || modals > 0) activeMenu.value = null
})

function toggleMenu(id) {
  activeMenu.value = activeMenu.value === id ? null : id
}
function hoverMenu(id) {
  if (activeMenu.value) activeMenu.value = id
}
function onMenuModel(id, value) {
  if (value) activeMenu.value = id
  else if (activeMenu.value === id) activeMenu.value = null
}
function onChoose(item) {
  activeMenu.value = null
  item.action?.()
}

function menuKeyNav(e, index) {
  if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') {
    e.preventDefault()
    const step = e.key === 'ArrowRight' ? 1 : -1
    const next = (index + step + MENU_LAYOUT.length) % MENU_LAYOUT.length
    activeMenu.value = MENU_LAYOUT[next].id
    menuButtons.value[next]?.focus()
  } else if (e.key === 'Escape') {
    activeMenu.value = null
    e.currentTarget?.blur?.()
  }
}

// 菜单展开时，←/→ 在顶层菜单间切换（事件从浮层冒泡到 nav）
function onMenubarKey(e) {
  if (!activeMenu.value) return
  if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
  e.preventDefault()
  const i = MENU_LAYOUT.findIndex((m) => m.id === activeMenu.value)
  const step = e.key === 'ArrowRight' ? 1 : -1
  const next = (i + step + MENU_LAYOUT.length) % MENU_LAYOUT.length
  activeMenu.value = MENU_LAYOUT[next].id
  menuButtons.value[next]?.focus()
}

// 菜单项里的开关动作（网格/吸附）走 store 日志口径
function toggleGrid(v) {
  S.ui.grid = v
  log(tr('log.grid', { v: tr(v ? 'log.on' : 'log.off') }))
}
function toggleSnap(v) {
  S.ui.snap = v
  log(tr('log.snap', { v: tr(v ? 'log.on' : 'log.off') }))
}

const projectTitle = computed(() =>
  S.project ? `${S.project.displayName || S.project.name}  —  ${S.project.path}` : tr('status.noProject'))

// 无框窗口：最大化/还原后同步图标与菜单文案
let stopResizeWatch = null
let maximizeTimer = null
onMounted(async () => {
  await refreshWindowState()
  stopResizeWatch = await onWindowResized(() => {
    clearTimeout(maximizeTimer)
    maximizeTimer = setTimeout(refreshWindowState, 80)
  })
})
onUnmounted(() => {
  stopResizeWatch?.()
  clearTimeout(maximizeTimer)
})
async function doToggleMaximize() {
  await toggleMaximizeWindow()
  await refreshWindowState()
}
</script>

<template>
  <header class="titlebar" data-tauri-drag-region>
    <div class="titlebar-brand" data-tauri-drag-region aria-hidden="true">
      <svg viewBox="0 0 20 20"><path d="M10 2l7 4v8l-7 4-7-4V6z" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round" /><path d="M10 6.5l3.5 2v3L10 13.5 6.5 11.5v-3z" fill="currentColor" opacity=".85" /></svg>
    </div>

    <nav class="menubar" role="menubar" aria-label="Main menu" @keydown="onMenubarKey">
      <ArDropdown
        v-for="(m, i) in MENU_LAYOUT"
        :key="m.id"
        :items="menuItems(m)"
        :open="activeMenu === m.id"
        :min-width="240"
        @update:open="onMenuModel(m.id, $event)"
        @choose="onChoose">
        <template #trigger="{ open }">
          <button
            :ref="(el) => (menuButtons[i] = el)"
            class="menubar-item"
            :class="{ open }"
            role="menuitem"
            :aria-expanded="open"
            @click.stop="toggleMenu(m.id)"
            @mouseenter="hoverMenu(m.id)"
            @keydown="menuKeyNav($event, i)">
            {{ m.label() }}
          </button>
        </template>
      </ArDropdown>
    </nav>

    <div class="titlebar-title" data-tauri-drag-region>{{ projectTitle }}</div>

    <div class="window-controls">
      <button class="win-btn" :title="tr('window.min')" :aria-label="tr('window.min')" @click="minimizeWindow">
        <svg viewBox="0 0 12 12"><path d="M2 6h8" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" /></svg>
      </button>
      <button
        class="win-btn win-max"
        :title="tr(windowState.maximized ? 'window.restore' : 'window.max')"
        :aria-label="tr(windowState.maximized ? 'window.restore' : 'window.max')"
        @click="doToggleMaximize">
        <svg v-if="!windowState.maximized" viewBox="0 0 12 12"><rect x="2.5" y="2.5" width="7" height="7" rx="1" fill="none" stroke="currentColor" stroke-width="1.2" /></svg>
        <svg v-else viewBox="0 0 12 12"><rect x="2.2" y="4.2" width="5.6" height="5.6" rx="1" fill="none" stroke="currentColor" stroke-width="1.1" /><path d="M4.4 4.2V3.3c0-.6.4-1 1-1h4.3c.6 0 1 .4 1 1v4.3c0 .6-.4 1-1 1h-.9" fill="none" stroke="currentColor" stroke-width="1.1" /></svg>
      </button>
      <button class="win-btn win-close" :title="tr('window.close')" :aria-label="tr('window.close')" @click="requestClose">
        <svg viewBox="0 0 12 12"><path d="M2.5 2.5l7 7M9.5 2.5l-7 7" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" /></svg>
      </button>
    </div>
  </header>

  <div class="toolbar" role="toolbar" aria-label="Editor toolbar">
    <ArInput
      size="sm"
      class="toolbar-search"
      data-global-search
      :model-value="S.ui.search"
      :placeholder="tr('cmd.searchPlaceholder')"
      @update:model-value="S.ui.search = $event"
      @enter="searchNext">
      <template #left>
        <svg class="tb-icon" viewBox="0 0 14 14" aria-hidden="true">
          <circle cx="6" cy="6" r="4.2" fill="none" stroke="currentColor" stroke-width="1.4" />
          <path d="M9.2 9.2L12 12" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
        </svg>
      </template>
    </ArInput>

    <span class="tb-divider" />

    <ArTooltip :text="tr('edit.undo') + '  Ctrl+Z'">
      <ArButton variant="ghost" size="sm" icon :disabled="!canUndo()" :aria-label="tr('edit.undo')" @click="doUndo">
        <svg class="tb-icon" viewBox="0 0 16 16"><path d="M6 4L2.5 7.5 6 11" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /><path d="M2.5 7.5h6.2A4 4 0 0 1 12.7 11.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></svg>
      </ArButton>
    </ArTooltip>
    <ArTooltip :text="tr('edit.redo') + '  Ctrl+Y'">
      <ArButton variant="ghost" size="sm" icon :disabled="!canRedo()" :aria-label="tr('edit.redo')" @click="doRedo">
        <svg class="tb-icon" viewBox="0 0 16 16"><g transform="translate(16 0) scale(-1 1)"><path d="M6 4L2.5 7.5 6 11" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /><path d="M2.5 7.5h6.2A4 4 0 0 1 12.7 11.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" /></g></svg>
      </ArButton>
    </ArTooltip>

    <span class="tb-divider" />

    <ArTooltip :text="tr('cmd.launch') + '  F6'">
      <ArButton variant="soft" size="sm" :disabled="!S.project" @click="commandById('run.launch').run()">
        <svg class="tb-icon" viewBox="0 0 16 16" aria-hidden="true"><path d="M5 3.5l7 4.5-7 4.5z" fill="currentColor" /></svg>
        {{ tr('cmd.launch') }}
      </ArButton>
    </ArTooltip>
    <ArTooltip :text="tr('cmd.stop')">
      <ArButton variant="ghost" size="sm" :disabled="!S.ui.gameRunning" @click="commandById('run.stop').run()">
        <svg class="tb-icon" viewBox="0 0 16 16" aria-hidden="true"><rect x="4" y="4" width="8" height="8" rx="1.5" fill="currentColor" /></svg>
        {{ tr('cmd.stop') }}
      </ArButton>
    </ArTooltip>

    <span class="tb-divider" />

    <ArSwitch size="sm" :label="tr('cmd.grid')" :model-value="S.ui.grid" @update:model-value="toggleGrid" />
    <ArSwitch size="sm" :label="tr('cmd.snap')" :model-value="S.ui.snap" @update:model-value="toggleSnap" />

    <span class="tb-spacer" />

    <ArTooltip :text="tr('cmd.validate') + '  F5'">
      <ArButton variant="outline" size="sm" @click="doValidate">
        <svg class="tb-icon" viewBox="0 0 16 16" aria-hidden="true"><path d="M8 1.8l5.5 2v4.4c0 3.2-2.4 5.4-5.5 6-3.1-.6-5.5-2.8-5.5-6V3.8z" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" /><path d="M5.6 8l1.7 1.7 3.2-3.4" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" /></svg>
        {{ tr('cmd.validate') }}
      </ArButton>
    </ArTooltip>
    <ArButton variant="primary" size="sm" @click="startBuild">
      <svg class="tb-icon" viewBox="0 0 16 16" aria-hidden="true"><path d="M3 12.5V6l5-3 5 3v6.5" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" /><path d="M6 12.5V9h4v3.5" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" /></svg>
      {{ tr('cmd.build') }}
    </ArButton>
  </div>

  <PathPicker
    :key="picker.step"
    v-model:current="picker.path"
    :open="picker.open"
    :mode="picker.mode"
    :title="picker.title"
    :filter="picker.filter"
    :initial-path="picker.initial"
    :confirm-disabled="pickerConfirmDisabled()"
    :busy="picker.busy"
    @close="closePicker"
    @confirm="onPickerConfirm">
    <template v-if="picker.step === 'new-parent'">
      <div class="pp-extra">
        <span class="fld-ar-label">{{ tr('picker.newName') }}</span>
        <ArInput size="sm" v-model="picker.newName" />
      </div>
      <p class="pp-extra-note" v-if="rootPreview()">{{ tr('picker.newRoot', { root: rootPreview() }) }}</p>
      <p class="pp-extra-note warn" v-else>{{ tr('picker.nameBad') }}</p>
    </template>
    <template v-else-if="picker.step === 'build'">
      <div class="pp-extra">
        <span class="fld-ar-label">{{ tr('picker.buildFormat') }}</span>
        <ArTabs
          size="sm"
          :tabs="[{ key: 'dir', label: tr('picker.folder') }, { key: 'zip', label: tr('picker.zip') }]"
          :model-value="picker.buildZip ? 'zip' : 'dir'"
          @update:model-value="picker.buildZip = $event === 'zip'" />
      </div>
    </template>
  </PathPicker>
</template>

<style scoped src="./menubar.css"></style>
<style scoped>
.tb-icon { width: 14px; height: 14px; flex: none; }
.pp-extra { display: flex; align-items: center; gap: var(--sp-3); }
.pp-extra > .fld-ar-label { width: 96px; }
.pp-extra .ar-field { flex: 1; min-width: 0; }
.pp-extra-note { color: var(--text-3); font-size: var(--fs-xs); }
.pp-extra-note.warn { color: var(--warn); }
</style>
