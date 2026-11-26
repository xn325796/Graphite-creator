// 真实窗口控制（Tauri 2）。capabilities 由后端在 src-tauri 侧放开
// core:window:allow-minimize / toggle-maximize / close / is-maximized。
// 浏览器（无 Tauri 运行时）下退化为 fullscreen / no-op，保证 vite dev 不崩。
import { reactive } from 'vue'
import { getCurrentWindow } from '@tauri-apps/api/window'

const inTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

/** 全局窗口状态：标题栏图标与菜单文案共用。 */
export const windowState = reactive({ maximized: false })

export async function minimizeWindow() {
  if (!inTauri()) return false
  try {
    await getCurrentWindow().minimize()
    return true
  } catch (e) {
    console.warn('[window] minimize failed', e)
    return false
  }
}

export async function toggleMaximizeWindow() {
  if (!inTauri()) {
    if (document.fullscreenElement) document.exitFullscreen?.()
    else document.documentElement.requestFullscreen?.()
    return true
  }
  try {
    await getCurrentWindow().toggleMaximize()
    return true
  } catch (e) {
    console.warn('[window] toggleMaximize failed', e)
    return false
  }
}

export async function closeWindow() {
  if (!inTauri()) {
    window.close()
    return false
  }
  try {
    await getCurrentWindow().close()
    return true
  } catch (e) {
    console.warn('[window] close failed', e)
    return false
  }
}

/** 当前是否最大化（浏览器下退化为全屏状态）。 */
export async function isWindowMaximized() {
  if (!inTauri()) return !!document.fullscreenElement
  try {
    return await getCurrentWindow().isMaximized()
  } catch (e) {
    console.warn('[window] isMaximized failed', e)
    return false
  }
}

/** 无框窗口的边缘缩放：direction 为 Tauri 的 ResizeDirection。 */
export async function startWindowResize(direction) {
  if (!inTauri()) return false
  try {
    await getCurrentWindow().startResizeDragging(direction)
    return true
  } catch (e) {
    console.warn('[window] startResizeDragging failed', e)
    return false
  }
}

/** 订阅窗口尺寸变化（最大化/还原/拖拽缩放都会触发），返回取消订阅函数。 */
export async function onWindowResized(cb) {
  if (!inTauri()) {
    const handler = () => cb()
    window.addEventListener('resize', handler)
    return () => window.removeEventListener('resize', handler)
  }
  try {
    return await getCurrentWindow().onResized(() => cb())
  } catch (e) {
    console.warn('[window] onResized failed', e)
    return () => {}
  }
}

/** 把最新最大化状态同步到 windowState。 */
export async function refreshWindowState() {
  windowState.maximized = await isWindowMaximized()
}
