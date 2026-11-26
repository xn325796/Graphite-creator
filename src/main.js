import { createApp } from "vue";
import "./styles/tokens.css";
import "./styles/base.css";
import App from "./App.vue";
import { initTheme } from "./lib/theme.js";

initTheme();
createApp(App).mount("#app");

// Desktop-shell interaction guards: keep browser behaviour out of the editor.
// Text selection stays available in inputs, textareas, selects and the log drawer.
const isEditable = (target) =>
  target instanceof Element &&
  !!target.closest('input, textarea, select, [contenteditable="true"], .log-drawer, .selectable');

window.addEventListener(
  "contextmenu",
  (event) => {
    if (!isEditable(event.target)) event.preventDefault();
  },
  true,
);
const isDraggable = (target) =>
  target instanceof Element && !!target.closest('[draggable="true"]');

window.addEventListener(
  "dragstart",
  (event) => {
    if (!isEditable(event.target) && !isDraggable(event.target)) event.preventDefault();
  },
  true,
);

window.addEventListener("mousedown", (event) => {
  if (event.button === 1) event.preventDefault();
});
window.addEventListener("auxclick", (event) => {
  if (event.button === 1) event.preventDefault();
});

// Dropping a file on a browser navigates away; the editor has no drop target yet.
window.addEventListener("dragover", (event) => event.preventDefault(), true);
window.addEventListener("drop", (event) => event.preventDefault(), true);

// Ctrl+wheel is browser zoom, not canvas zoom (canvas uses plain wheel).
window.addEventListener(
  "wheel",
  (event) => {
    if (event.ctrlKey || event.metaKey) event.preventDefault();
  },
  { passive: false },
);

// Block browser-only shortcuts (reload, print, view-source, zoom, history).
window.addEventListener(
  "keydown",
  (event) => {
    if (event.altKey && (event.key === "ArrowLeft" || event.key === "ArrowRight")) {
      event.preventDefault();
      return;
    }
    if (!(event.ctrlKey || event.metaKey)) return;
    const key = event.key.toLowerCase();
    if (["r", "p", "u", "+", "-", "=", "0"].includes(key)) {
      event.preventDefault();
    }
  },
  true,
);
