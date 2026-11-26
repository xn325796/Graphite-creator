// 可拖拽面板宽度：拖右缘（edge='right'）或左缘（edge='left'），双击复位，本地持久化。
import { onUnmounted, ref } from 'vue'

const clamp = (v, min, max) => Math.min(max, Math.max(min, Math.round(v)))

export const RESET_PANELS_EVENT = 'graphite:reset-panels'

/** 重置所有已注册面板的宽度（菜单“视图 ▸ 重置面板宽度”调用）。 */
export function resetAllPanelWidths() {
  window.dispatchEvent(new CustomEvent(RESET_PANELS_EVENT))
}

export function usePanelWidth({ key, initial, min = 180, max = 480, edge = 'right' }) {
  const read = () => {
    try {
      const raw = Number(localStorage.getItem(key))
      if (Number.isFinite(raw) && raw > 0) return clamp(raw, min, max)
    } catch { /* 隐私模式等场景忽略 */ }
    return clamp(initial, min, max)
  }

  const width = ref(read())
  const dragging = ref(false)
  const dir = edge === 'right' ? 1 : -1

  function startResize(e) {
    if (e.button !== 0) return
    e.preventDefault()
    e.stopPropagation()
    dragging.value = true
    const startX = e.clientX
    const startW = width.value
    document.body.style.cursor = 'col-resize'

    const move = (ev) => {
      width.value = clamp(startW + dir * (ev.clientX - startX), min, max)
    }
    const up = () => {
      dragging.value = false
      document.body.style.cursor = ''
      window.removeEventListener('pointermove', move)
      window.removeEventListener('pointerup', up)
      try { localStorage.setItem(key, String(width.value)) } catch { /* noop */ }
    }
    window.addEventListener('pointermove', move)
    window.addEventListener('pointerup', up)
  }

  function reset() {
    width.value = clamp(initial, min, max)
    try { localStorage.removeItem(key) } catch { /* noop */ }
  }

  // 全局重置：每个面板实例各自订阅，卸载时自动解绑
  if (typeof window !== 'undefined') {
    window.addEventListener(RESET_PANELS_EVENT, reset)
    onUnmounted(() => window.removeEventListener(RESET_PANELS_EVENT, reset))
  }

  return { width, dragging, startResize, reset }
}
