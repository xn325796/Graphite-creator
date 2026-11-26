import { reactive } from 'vue'

// ArModal 实例计数：任一有模态开启时，MenuBar 的全局快捷键让位（焦点陷阱在 ArModal 内）。
export const modalState = reactive({ open: 0 })