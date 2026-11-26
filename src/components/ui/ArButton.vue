<script setup>
defineProps({
  /** primary | secondary | soft | ghost | outline | danger */
  variant: { type: String, default: 'secondary' },
  size: { type: String, default: 'md' },
  disabled: Boolean,
  icon: Boolean,
  block: Boolean,
  type: { type: String, default: 'button' },
})
</script>

<template>
  <button
    class="ar-btn"
    :class="[`ar-btn-${variant}`, `ar-btn-${size}`, { 'ar-btn-icon': icon, 'ar-btn-block': block }]"
    :type="type"
    :disabled="disabled"
  >
    <slot />
  </button>
</template>

<style scoped>
.ar-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: var(--sp-2);
  height: var(--h-control-md);
  padding: 0 var(--sp-5);
  border: 1px solid transparent;
  border-radius: var(--r-sm);
  background: var(--bg-elev);
  color: var(--text);
  font-family: inherit;
  font-size: var(--fs-sm);
  font-weight: 500;
  line-height: 1;
  white-space: nowrap;
  transition: background var(--dur-fast) var(--ease), border-color var(--dur-fast) var(--ease),
    color var(--dur-fast) var(--ease), box-shadow var(--dur-fast) var(--ease);
}

.ar-btn:hover:not(:disabled) { background: var(--bg-elev-2); }
.ar-btn:active:not(:disabled) { background: var(--bg-active); }
.ar-btn:focus-visible { outline: none; box-shadow: var(--ring); }
.ar-btn:disabled { opacity: 0.42; cursor: default; }

.ar-btn-xs { height: var(--h-control-xs); padding: 0 var(--sp-3); font-size: var(--fs-xs); }
.ar-btn-sm { height: var(--h-control-sm); padding: 0 var(--sp-4); font-size: var(--fs-xs); }
.ar-btn-md { height: var(--h-control-md); padding: 0 var(--sp-5); }

.ar-btn-icon { width: var(--h-control-md); padding: 0; }
.ar-btn-icon.ar-btn-sm { width: var(--h-control-sm); }
.ar-btn-icon.ar-btn-xs { width: var(--h-control-xs); }
.ar-btn-block { width: 100%; }

/* primary：品牌色实底，深色文字 */
.ar-btn-primary {
  background: var(--accent-strong);
  border-color: transparent;
  color: var(--accent-ink);
  font-weight: 600;
}
.ar-btn-primary:hover:not(:disabled) { background: var(--accent); }
.ar-btn-primary:active:not(:disabled) { background: #24bda9; }

/* secondary：描边中性按钮（默认） */
.ar-btn-secondary {
  background: var(--bg-elev);
  border-color: var(--border);
}
.ar-btn-secondary:hover:not(:disabled) { background: var(--bg-elev-2); border-color: var(--border-strong); }

/* soft：低对比填充 */
.ar-btn-soft {
  background: rgba(255, 255, 255, 0.06);
  color: var(--text);
}
.ar-btn-soft:hover:not(:disabled) { background: var(--bg-hover); }

/* ghost：仅 hover 有底 */
.ar-btn-ghost {
  background: transparent;
  color: var(--text-2);
}
.ar-btn-ghost:hover:not(:disabled) { background: var(--bg-hover); color: var(--text); }

/* outline：透明底 + 可见描边 */
.ar-btn-outline {
  background: transparent;
  border-color: var(--border-strong);
  color: var(--text);
}
.ar-btn-outline:hover:not(:disabled) { border-color: var(--accent-line); color: var(--accent); }

/* danger */
.ar-btn-danger {
  background: var(--danger-soft);
  color: var(--danger);
}
.ar-btn-danger:hover:not(:disabled) { background: rgba(248, 113, 113, 0.22); }
</style>
