import { invoke as rawInvoke } from '@tauri-apps/api/core'
import { listen as rawListen } from '@tauri-apps/api/event'

export function invoke(cmd, args = {}) {
  return rawInvoke(cmd, args)
}

// listen 只在挂载期调用一次；浏览器（vite dev 无 Tauri 运行时）下静默降级，
// 避免 onMounted 因同步 TypeError 中断导致整个界面空白。
export function listen(event, cb) {
  try {
    const p = rawListen(event, (e) => cb(e.payload))
    if (p && typeof p.catch === 'function') p.catch(() => {})
    return p
  } catch {
    return Promise.resolve(() => {})
  }
}

export const errCode = (e) => (e && e.code) || 'unknown'
export const errMsg = (e) => (e && e.message) || String(e)
export const isNoProject = (e) => errCode(e) === 'no-project'
