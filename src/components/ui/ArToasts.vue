<script setup>
import { toasts, dismissToast } from '../../lib/toast.js'
</script>

<template>
  <Teleport to="body">
    <div class="ar-toasts" aria-live="polite">
      <transition-group name="ar-toast">
        <div v-for="t in toasts" :key="t.id" class="ar-toast" :class="`ar-toast-${t.kind}`" @click="dismissToast(t.id)">
          <span class="ar-toast-dot"></span>
          <span class="ar-toast-msg">{{ t.message }}</span>
        </div>
      </transition-group>
    </div>
  </Teleport>
</template>

<style scoped>
.ar-toasts {
  position: fixed;
  right: var(--sp-5);
  bottom: calc(var(--h-status) + var(--sp-4));
  z-index: var(--z-toast);
  display: flex;
  flex-direction: column;
  gap: var(--sp-3);
  align-items: flex-end;
  pointer-events: none;
}
.ar-toast {
  display: flex;
  align-items: center;
  gap: var(--sp-3);
  max-width: 420px;
  padding: var(--sp-3) var(--sp-4);
  background: var(--bg-elev);
  border: 1px solid var(--border-strong);
  border-radius: var(--r-md);
  box-shadow: var(--shadow-pop);
  color: var(--text);
  font-size: var(--fs-sm);
  pointer-events: auto;
  cursor: pointer;
}
.ar-toast-dot { width: 8px; height: 8px; border-radius: 50%; flex: none; }
.ar-toast-info .ar-toast-dot { background: var(--accent); }
.ar-toast-warn .ar-toast-dot { background: var(--warn); }
.ar-toast-err .ar-toast-dot { background: var(--danger); }
.ar-toast-msg { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

.ar-toast-enter-active { transition: opacity var(--dur) var(--ease), transform var(--dur) var(--ease); }
.ar-toast-leave-active { transition: opacity var(--dur-fast) ease; }
.ar-toast-enter-from { opacity: 0; transform: translateY(6px); }
.ar-toast-leave-to { opacity: 0; }
</style>
