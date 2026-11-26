// 主题：深色 / 浅色 / 跟随系统。
// 只切换 <html data-theme>，颜色全部由 tokens.css 的语义变量承担。
import { reactive } from 'vue'

const KEY = 'graphite.theme'
const media = typeof window !== 'undefined' && window.matchMedia

const read = () => {
  try {
    const v = localStorage.getItem(KEY)
    return v === 'light' || v === 'dark' || v === 'system' ? v : 'system'
  } catch { return 'system' }
}

export const themeState = reactive({ mode: read() })

const systemTheme = () => (media && media('(prefers-color-scheme: light)').matches ? 'light' : 'dark')

/** 当前实际生效的主题（mode 为 system 时解析系统设置）。 */
export const resolvedTheme = () => (themeState.mode === 'system' ? systemTheme() : themeState.mode)

function apply() {
  document.documentElement.dataset.theme = resolvedTheme()
}

export function setTheme(mode) {
  themeState.mode = mode === 'light' || mode === 'dark' ? mode : 'system'
  try { localStorage.setItem(KEY, themeState.mode) } catch { /* noop */ }
  apply()
}

export function initTheme() {
  apply()
  if (!media) return
  media('(prefers-color-scheme: light)').addEventListener('change', () => {
    if (themeState.mode === 'system') apply()
  })
}
