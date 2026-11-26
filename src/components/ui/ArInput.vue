<script setup>
import { useId, ref } from 'vue'

const model = defineModel({ type: String, default: '' })
const props = defineProps({
  label: String,
  placeholder: { type: String, default: ' ' },
  invalid: Boolean,
  disabled: Boolean,
  readonly: Boolean,
  mono: Boolean,
  size: { type: String, default: 'md' },
  type: { type: String, default: 'text' },
})
const emit = defineEmits(['enter', 'blur', 'input'])
const uid = useId()
const inputEl = ref(null)
const preEdit = ref('')
let selectTimer = null

defineExpose({ focus: () => inputEl.value?.focus(), select: () => inputEl.value?.select() })

function onFocus() {
  preEdit.value = model.value
  clearTimeout(selectTimer)
  // 等 mouseup 之后再全选，否则点击会把选区折叠掉；同时确认焦点仍在本框，
  // 避免本次全选（select() 会抢焦点）把刚打开的对话框/命令面板顶掉。
  selectTimer = setTimeout(() => {
    const el = inputEl.value
    if (!el || document.activeElement !== el) return
    try { el.setSelectionRange(0, el.value.length) } catch { el.select?.() }
  }, 0)
}

function onKeydown(e) {
  if (e.key !== 'Escape') return
  // 模态内保持 Esc = 关闭对话框（路径选择等）；检查器字段则回退编辑值
  if (e.target?.closest?.('.ar-modal')) return
  const prev = preEdit.value
  if (model.value !== prev) {
    model.value = prev
    // 兼容 :model-value + @input 的受控用法（检查器/筛选框），让上层回到原值
    emit('input', { target: { value: prev } })
  }
  e.target?.blur?.()
}
</script>

<template>
  <label
    class="ar-field"
    :class="[`ar-field-${props.size}`, { 'ar-field-invalid': invalid, 'ar-field-disabled': disabled, 'ar-field-filled': model !== '', 'ar-field-label': !!props.label }]"
    :for="uid"
  >
    <span class="ar-field-shell">
      <slot name="left" />
      <input
        :id="uid"
        ref="inputEl"
        v-model="model"
        class="ar-input"
        :class="{ 'ar-input-mono': mono }"
        :type="props.type"
        :placeholder="props.placeholder === ' ' && props.label ? ' ' : props.placeholder"
        :disabled="disabled"
        :readonly="readonly"
        :aria-invalid="invalid || undefined"
        @focus="onFocus"
        @keydown="onKeydown"
        @input="emit('input', $event)"
        @blur="emit('blur', $event)"
        @keydown.enter="emit('enter', $event)"
      />
      <span v-if="props.label" class="ar-label">{{ props.label }}</span>
      <slot name="right" />
    </span>
  </label>
</template>

<style scoped>
.ar-field { display: block; min-width: 0; }

.ar-field-shell {
  position: relative;
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  height: var(--h-control-md);
  padding: 0 var(--sp-4);
  border: 1px solid var(--border);
  border-radius: var(--r-sm);
  background: var(--bg-field);
  transition: border-color var(--dur-fast) var(--ease), box-shadow var(--dur-fast) var(--ease),
    background var(--dur-fast) var(--ease);
}

.ar-field-shell:hover { border-color: var(--border-strong); }
.ar-field-shell:focus-within {
  border-color: var(--accent-strong);
  box-shadow: 0 0 0 3px var(--accent-soft);
  background: var(--bg-canvas);
}
.ar-field-sm .ar-field-shell { height: var(--h-control-sm); padding: 0 var(--sp-3); }
.ar-field-xs .ar-field-shell { height: var(--h-control-xs); padding: 0 var(--sp-2); }

.ar-field-invalid .ar-field-shell { border-color: var(--danger); }
.ar-field-invalid .ar-field-shell:focus-within { box-shadow: 0 0 0 3px var(--danger-soft); }
.ar-field-disabled .ar-field-shell { opacity: 0.5; }

.ar-input {
  flex: 1;
  min-width: 0;
  height: 100%;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--text);
  font-family: inherit;
  font-size: var(--fs-sm);
  padding: 0;
}
.ar-input-mono { font-family: var(--font-mono); font-size: var(--fs-xs); }
.ar-input::placeholder { color: var(--text-3); }
.ar-field-filled .ar-input::placeholder { color: transparent; }

/* 浮动标签 */
.ar-label {
  position: absolute;
  left: var(--sp-3);
  top: 50%;
  transform: translateY(-50%);
  padding: 0 var(--sp-1);
  background: var(--bg-field);
  color: var(--text-3);
  font-size: var(--fs-sm);
  pointer-events: none;
  transition: top var(--dur) var(--ease), font-size var(--dur) var(--ease), color var(--dur) var(--ease);
}
.ar-field-shell:focus-within .ar-label,
.ar-field:focus-within .ar-label { background: var(--bg-canvas); }
.ar-field:focus-within .ar-label,
.ar-field-filled .ar-label {
  top: 0;
  font-size: var(--fs-2xs);
  color: var(--accent);
}
.ar-field-invalid .ar-label { color: var(--danger); }
</style>
