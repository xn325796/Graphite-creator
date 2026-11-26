<script setup>
// 无框窗口的四边/四角缩放热区。
// Tauri 在 Windows 上不保证无框窗口自带可缩放边框，这里显式接管。
import { startWindowResize } from '../../lib/window.js'

const zones = [
  { key: 'n', dir: 'North' },
  { key: 's', dir: 'South' },
  { key: 'w', dir: 'West' },
  { key: 'e', dir: 'East' },
  { key: 'nw', dir: 'NorthWest' },
  { key: 'ne', dir: 'NorthEast' },
  { key: 'sw', dir: 'SouthWest' },
  { key: 'se', dir: 'SouthEast' },
]
</script>

<template>
  <div class="window-resizers" aria-hidden="true">
    <div
      v-for="z in zones"
      :key="z.key"
      class="window-resize"
      :class="`rz-${z.key}`"
      @pointerdown.prevent="startWindowResize(z.dir)" />
  </div>
</template>

<style scoped>
.window-resizers {
  position: fixed;
  inset: 0;
  z-index: 60;
  pointer-events: none;
}

.window-resize {
  position: fixed;
  pointer-events: auto;
  touch-action: none;
}

.rz-n { top: 0; left: 10px; right: 10px; height: 5px; cursor: ns-resize; }
.rz-s { bottom: 0; left: 10px; right: 10px; height: 5px; cursor: ns-resize; }
.rz-w { left: 0; top: 10px; bottom: 10px; width: 5px; cursor: ew-resize; }
.rz-e { right: 0; top: 10px; bottom: 10px; width: 5px; cursor: ew-resize; }

.rz-nw { left: 0; top: 0; width: 10px; height: 10px; cursor: nwse-resize; }
.rz-se { right: 0; bottom: 0; width: 10px; height: 10px; cursor: nwse-resize; }
.rz-ne { right: 0; top: 0; width: 10px; height: 10px; cursor: nesw-resize; }
.rz-sw { left: 0; bottom: 0; width: 10px; height: 10px; cursor: nesw-resize; }
</style>
