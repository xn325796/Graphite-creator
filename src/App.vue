<script setup>
import Home from './views/home.vue'
import WindowResizer from './components/layout/WindowResizer.vue'
</script>

<template>
  <div class="stage">
    <Home />
    <WindowResizer />
  </div>
</template>

<style>
.stage {
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
  position: relative;
  overflow: hidden;
  background: var(--bg-app);
}
</style>
