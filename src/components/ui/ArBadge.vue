<script setup>
defineProps({
  variant: { type: String, default: 'neutral' },
})
</script>

<template>
  <span class="ar-badge" :class="`ar-badge-${variant}`"><slot /></span>
</template>

<style scoped>
.ar-badge {
  display: inline-flex;
  align-items: center;
  gap: var(--sp-1);
  height: 18px;
  padding: 0 var(--sp-2);
  border: 1px solid transparent;
  border-radius: var(--r-sm);
  font-size: var(--fs-2xs);
  font-weight: 500;
  letter-spacing: 0.02em;
  white-space: nowrap;
}
.ar-badge-accent { background: var(--accent-soft); color: var(--accent); border-color: rgba(94, 234, 212, 0.24); }
.ar-badge-warn { background: var(--warn-soft); color: var(--warn); border-color: rgba(251, 191, 36, 0.24); }
.ar-badge-err { background: var(--danger-soft); color: var(--danger); border-color: rgba(248, 113, 113, 0.24); }
.ar-badge-ok { background: var(--ok-soft); color: var(--ok); border-color: rgba(52, 211, 153, 0.24); }
.ar-badge-neutral { background: rgba(255, 255, 255, 0.07); color: var(--text-2); }
</style>
