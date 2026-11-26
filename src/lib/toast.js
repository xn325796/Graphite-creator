import { reactive } from 'vue'

export const toasts = reactive([])
let nextId = 0

export function toast(message, kind = 'info', ms = 3600) {
  const item = { id: ++nextId, message, kind }
  toasts.push(item)
  if (toasts.length > 3) toasts.shift()
  setTimeout(() => {
    const i = toasts.indexOf(item)
    if (i >= 0) toasts.splice(i, 1)
  }, ms)
}

export function dismissToast(id) {
  const i = toasts.findIndex((t) => t.id === id)
  if (i >= 0) toasts.splice(i, 1)
}
