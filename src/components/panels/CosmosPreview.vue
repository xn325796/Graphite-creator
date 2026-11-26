<script setup>
// Canvas 2D 星球预览：区块网格 / 星球外观 / 星系位置。
// 行为对齐 Java ui/PlanetPreview.java；数据全部来自 cosmos_scene(recordId) 契约。
import { onMounted, onUnmounted, ref, watch } from 'vue'
import {
  buildAtmosphereSprite, clamp, clamp01, drawStars, faceNormal, hexColor,
  lambert, lightFromSystem, maxLayerLength, normalize, parseHexColor, rgba, rotate, rotateY,
} from '../../lib/cosmosRender.js'

const props = defineProps({
  scene: { type: Object, default: null },
  tab: { type: Number, default: 0 },
  highlightId: { type: String, default: null },
  label: { type: String, default: '' },
  vanillaLabel: { type: String, default: '' },
})

const wrapRef = ref(null)
const canvasRef = ref(null)
const dragging = ref(false)

// GPU 可用时（默认 ANGLE D3D11）保持全分辨率；只有 WebView2 回退到软件光栅
// （--disable-gpu 或驱动失效）才启用分辨率上限与拖拽 LOD。
const SOFTWARE_RASTER = (() => {
  try {
    const probe = document.createElement('canvas')
    const gl = probe.getContext('webgl2') || probe.getContext('webgl')
    if (!gl) return true
    const ext = gl.getExtension('WEBGL_debug_renderer_info')
    const name = ext ? String(gl.getParameter(ext.UNMASKED_RENDERER_WEBGL)) : ''
    return /swiftshader|basic render|llvmpipe|software/i.test(name)
  } catch {
    return true
  }
})()

let yaw = 0.7
let pitch = 0.35
let dragX = 0
let dragY = 0
let raf = 0
let timer = null
let cloudTime = 0
let atmosphereCache = { key: '', sprite: null }
const size = { w: 1, h: 1, sx: 1, sy: 1 }

// WebView2 在这台 Win11 Insider 上必须带 --disable-gpu（否则 GPU 进程直接退出），
// Canvas 是软件光栅。把 backing store 限制在 640×480，拖拽时再降到 320×240；
// CSS 仍铺满容器，视觉上只是轻微变软，但能把 1600×900 的 828×645 填充量降到 1/3。
const FULL_MAX_W = 640
const FULL_MAX_H = 480
const DRAG_MAX_W = 320
const DRAG_MAX_H = 240

let ro = null
let ctx = null

function measure(quality = 1) {
  const wrap = wrapRef.value
  const canvas = canvasRef.value
  if (!wrap || !canvas) return
  const rect = wrap.getBoundingClientRect()
  const w = Math.max(1, Math.round(rect.width))
  const h = Math.max(1, Math.round(rect.height))
  const dpr = Math.min(2, window.devicePixelRatio || 1)
  const maxW = SOFTWARE_RASTER ? (quality < 1 ? DRAG_MAX_W : FULL_MAX_W) : Infinity
  const maxH = SOFTWARE_RASTER ? (quality < 1 ? DRAG_MAX_H : FULL_MAX_H) : Infinity
  const scale = SOFTWARE_RASTER
    ? Math.min(1, maxW / (w * dpr), maxH / (h * dpr))
    : 1
  size.w = w
  size.h = h
  size.sx = dpr * scale
  size.sy = dpr * scale
  const pw = Math.max(1, Math.round(w * size.sx))
  const ph = Math.max(1, Math.round(h * size.sy))
  if (canvas.width !== pw) canvas.width = pw
  if (canvas.height !== ph) canvas.height = ph
  ctx = canvas.getContext('2d')
  if (ctx) ctx.setTransform(size.sx, 0, 0, size.sy, 0, 0)
}

// 可见时用 rAF 合并；页面隐藏（最小化/后台）时 rAF 会被浏览器暂停，
// 离散更新改为同步绘制，避免切回窗口时看到空白画布。
function requestDraw(immediate = false) {
  if (immediate || document.hidden) {
    if (raf) { cancelAnimationFrame(raf); raf = 0 }
    draw()
    return
  }
  if (raf) return
  raf = requestAnimationFrame(() => {
    raf = 0
    draw()
  })
}

function draw() {
  if (!ctx) measure()
  if (!ctx) return
  const { w, h } = size
  ctx.setTransform(size.sx, 0, 0, size.sy, 0, 0)
  ctx.clearRect(0, 0, w, h)
  ctx.fillStyle = '#0e1013'
  ctx.fillRect(0, 0, w, h)
  const scene = props.scene
  if (!scene) return
  if (props.tab === 0) drawGrid(scene, w, h)
  else if (props.tab === 1) {
    // Java drawAppearance 画星空；drawOrbit 不画（PlanetPreview.java:486-489, 836-838）。
    if (!dragging.value || !SOFTWARE_RASTER) drawStars(ctx, w, h)
    drawAppearance(scene, w, h, dragging.value && SOFTWARE_RASTER)
  } else {
    drawSystem(scene, w, h)
  }
}

// ---------- 区块网格 ----------

function drawGrid(scene, w, h) {
  const grid = scene.grid
  if (!grid) return
  const scale = Math.min(w, h) * 0.42
  const cx = w / 2
  const cy = h / 2
  ctx.save()
  ctx.strokeStyle = '#343a41'
  ctx.lineWidth = 1
  ctx.beginPath()
  ctx.arc(cx, cy, scale, 0, Math.PI * 2)
  ctx.stroke()

  const tiles = grid.tiles || []
  for (const tile of tiles) {
    const center = tile.center || [0, 0, 1]
    const front = rotate(center[0], center[1], center[2], yaw, pitch)[2] > 0
    const pts = projectCorners(tile.corners || [], 0, scale, cx, cy)
    if (!pts) continue
    if (front) {
      ctx.fillStyle = '#24282d'
      ctx.strokeStyle = '#3f464e'
      ctx.fill(polygon(pts))
      ctx.stroke(polygon(pts))
    } else {
      ctx.strokeStyle = '#1d2024'
      ctx.stroke(polygon(pts))
    }
  }

  for (let i = 0; i < tiles.length; i++) {
    const tile = tiles[i]
    if (!tile.occupied && !(tile.sectorIds && tile.sectorIds.length)) continue
    const center = tile.center || [0, 0, 1]
    const front = rotate(center[0], center[1], center[2], yaw, pitch)[2] > 0
    const pts = projectCorners(tile.corners || [], 0, scale, cx, cy)
    if (!pts) continue
    const selected = !!props.highlightId && (tile.sectorIds || []).includes(props.highlightId)
    ctx.strokeStyle = selected ? '#ffffff' : '#b47df0'
    ctx.lineWidth = selected ? 2 : 1.2
    ctx.stroke(polygon(pts))
    if (front) {
      ctx.fillStyle = selected ? '#8455bd' : rgba(0.706, 0.49, 0.94, 0.75)
      ctx.fill(polygon(pts))
      ctx.fillStyle = '#ffffff'
      ctx.font = 'bold 12px "Inter", "Microsoft YaHei", sans-serif'
      ctx.textAlign = 'left'
      ctx.textBaseline = 'alphabetic'
      ctx.fillText(String(tile.index ?? i), pts.textX - 5, pts.textY + 4)
    }
  }
  ctx.restore()
}

// ---------- 星球外观 ----------

function drawAppearance(scene, w, h, lite = false) {
  const appearance = scene.appearance
  const surface = appearance?.surface || []
  if (!surface.length) return
  const cx = w / 2
  const cy = h / 2
  let maxExtent = appearance.maxExtent || 1
  for (const layer of surface) maxExtent = Math.max(maxExtent, maxLayerLength(layer))
  const scale = Math.min(w, h) * 0.42 / maxExtent
  const light = appearance.light?.length === 3
    ? normalize(appearance.light[0], appearance.light[1], appearance.light[2])
    : lightFromSystem(scene.system?.bodies)

  let surfaceRadius = 0
  for (const layer of surface) surfaceRadius = Math.max(surfaceRadius, layer.radius ?? 1)

  ctx.save()
  for (const layer of surface) {
    if ((layer.radius ?? 1) < surfaceRadius * 0.98) continue
    drawLayer(layer, light, cx, cy, scale)
  }
  for (const layer of appearance.clouds || []) {
    if ((layer.radius ?? 1) < surfaceRadius * 0.98) continue
    drawLayer(layer, light, cx, cy, scale)
  }

  if (!lite) {
    drawAtmosphere(appearance, maxExtent, scale, light, cx, cy)
    drawOccupiedOutlines(scene.grid, cx, cy, maxExtent * scale * 1.04)
  }
  ctx.restore()
}

function drawLayer(layer, light, cx, cy, scale) {
  const cloudYaw = layer.sky && layer.cloudSpeed ? cloudTime * layer.cloudSpeed / 40 : 0
  const tileCount = layer.tiles?.length || 0
  // 只有软件光栅 + 超大网格才放弃逐块 stroke；GPU 上保持 Java 的封缝行为。
  const seal = !SOFTWARE_RASTER || tileCount <= 1500
  for (const tile of layer.tiles || []) {
    if (tile.skipped) continue
    const color = tile.color
    if (!color || color.length < 4 || color[3] <= 0.01) continue
    const corners = tile.corners || []
    const pts = projectCorners(corners, cloudYaw, scale, cx, cy)
    if (!pts || pts.centroidZ <= 0) continue
    const fill = shade(layer, corners, color, light)
    ctx.fillStyle = fill
    ctx.strokeStyle = fill
    ctx.lineWidth = 1
    const path = polygon(pts)
    ctx.fill(path)
    if (seal) ctx.stroke(path)
  }
}

function shade(layer, corners, color, light) {
  const [r, g, b, a] = shadeParts(layer, corners, color, light)
  return rgba(r, g, b, a)
}

function shadeParts(layer, corners, color, light) {
  const [r, g, b, a] = color
  if (layer.unlit) return [r, g, b, 1]
  if (layer.sky) {
    // clouds.vert: v_col = a_color * (0.01 + halfLambert)
    const half = clamp01((lambert(corners, light, yaw, pitch) + 1) / 2)
    const norc = 0.01 + half
    return [r * norc, g * norc, b * norc, a]
  }
  // planet.vert: norc = (1 + specular) * (0.01 + halfLambert)，alpha 强制 1
  const n = faceNormal(corners, yaw, pitch)
  const lam = n[0] * light[0] + n[1] * light[1] + n[2] * light[2]
  const diffuse = clamp01((lam + 1) / 2)
  const albedo = 1 - a
  const rz = n[2] - 2 * lam * light[2] // reflect(N, L).z；正交相机视线 (0,0,1)
  const specular = rz > 0 ? Math.pow(rz, 40) * albedo : 0
  const norc = (1 + specular) * (0.01 + diffuse)
  return [r * norc, g * norc, b * norc, 1]
}

function drawAtmosphere(appearance, maxExtent, scale, light, cx, cy) {
  const atm = appearance.atmosphere
  if (!atm || !(atm.radOut > 0)) return
  // 量化缩放，避免窗口连续缩放时每像素都重算大气
  const atmScale = Math.max(1, Math.round(scale / 4) * 4)
  const key = [
    appearance.meshType || '',
    atm.color || '',
    atm.radOut,
    maxExtent.toFixed(3),
    atmScale,
    light.map((v) => v.toFixed(3)).join(','),
  ].join('|')
  if (atmosphereCache.key !== key) {
    atmosphereCache = {
      key,
      sprite: buildAtmosphereSprite(atm.color, atm.radOut, maxExtent, atmScale, light),
    }
  }
  const sprite = atmosphereCache.sprite
  if (!sprite) return
  ctx.globalCompositeOperation = 'lighter'
  ctx.drawImage(sprite.canvas, cx - sprite.size / 2, cy - sprite.size / 2)
  ctx.globalCompositeOperation = 'source-over'
}

function drawOccupiedOutlines(grid, cx, cy, scale) {
  if (!grid) return
  ctx.strokeStyle = rgba(0.706, 0.49, 0.94, 0.8)
  ctx.lineWidth = 1.1
  for (const tile of grid.tiles || []) {
    if (!tile.occupied && !(tile.sectorIds && tile.sectorIds.length)) continue
    const center = tile.center || [0, 0, 1]
    if (rotate(center[0], center[1], center[2], yaw, pitch)[2] <= 0) continue
    const pts = projectCorners(tile.corners || [], 0, scale, cx, cy)
    if (pts) ctx.stroke(polygon(pts))
  }
}

// ---------- 星系位置 ----------

function drawSystem(scene, w, h) {
  const bodies = scene.system?.bodies || []
  if (!bodies.length) return
  const sunColors = scene.system?.sunColors || ['ff7a38', 'ff9638', 'ffc64c', 'ffe371']
  let maxOrbit = 1
  for (const body of bodies) maxOrbit = Math.max(maxOrbit, body.orbitRadius || 0)
  const scale = Math.min(w, h) * 0.42 / maxOrbit
  const cx = w / 2
  const cy = h / 2

  ctx.save()
  ctx.strokeStyle = '#343a41'
  ctx.lineWidth = 1
  ctx.setLineDash([4, 4])
  for (const body of bodies) {
    if ((body.orbitRadius || 0) > 0) {
      ctx.beginPath()
      ctx.arc(cx, cy, body.orbitRadius * scale, 0, Math.PI * 2)
      ctx.stroke()
    }
  }
  ctx.setLineDash([])

  let index = 0
  for (const body of bodies) {
    if (body.star) {
      const r = Math.max(8, (body.radius ?? 1) / maxOrbit * scale)
      ctx.fillStyle = hexColor(sunColors[2], '#ffc64c')
      ctx.beginPath()
      ctx.arc(cx, cy, r, 0, Math.PI * 2)
      ctx.fill()
      label(body.name, cx, cy - r - 6, body.selected)
      index++
      continue
    }
    const angle = 0.6 + index * (Math.PI * 2) / 5
    index++
    const px = cx + Math.cos(angle) * (body.orbitRadius || 0) * scale
    const py = cy - Math.sin(angle) * (body.orbitRadius || 0) * scale
    const dot = Math.max(4, Math.min(10, (body.radius ?? 1) / maxOrbit * scale * 3))
    const fill = body.color
      ? hexColor(body.color)
      : (body.selected ? '#b47df0' : '#5f666e')
    ctx.fillStyle = fill
    ctx.beginPath()
    ctx.arc(px, py, dot, 0, Math.PI * 2)
    ctx.fill()
    if (body.selected) {
      ctx.strokeStyle = '#ffffff'
      ctx.lineWidth = 1.5
      ctx.beginPath()
      ctx.arc(px, py, dot + 3, 0, Math.PI * 2)
      ctx.stroke()
    }
    label(body.name + (body.vanilla ? ` (${props.vanillaLabel || 'vanilla'})` : ''), px, py - dot - 5, body.selected)
  }
  ctx.restore()
}

function label(text, x, y, emphasized) {
  if (!text) return
  ctx.fillStyle = emphasized ? '#ffffff' : '#9aa1a9'
  ctx.font = `${emphasized ? 'bold ' : ''}11px "Inter", "Microsoft YaHei", sans-serif`
  ctx.textAlign = 'left'
  ctx.textBaseline = 'alphabetic'
  // Keep labels inside the canvas: a selected body with a large explicit orbit
  // (e.g. orbitRadius 40) would otherwise draw its name off the right edge.
  const width = ctx.measureText(text).width
  const lx = Math.min(Math.max(x - 18, 4), Math.max(4, size.w - width - 4))
  const ly = Math.min(Math.max(y, 12), size.h - 4)
  ctx.fillText(text, lx, ly)
}

// ---------- 几何 ----------

function projectCorners(corners, cloudYaw, scale, cx, cy) {
  const points = Math.floor(corners.length / 3)
  if (points < 3) return null
  const xs = new Array(points)
  const ys = new Array(points)
  let centroidZ = 0
  for (let k = 0; k < points; k++) {
    let x = corners[k * 3]
    const y = corners[k * 3 + 1]
    let z = corners[k * 3 + 2]
    if (cloudYaw) {
      const rotated = rotateY(x, y, z, cloudYaw)
      x = rotated[0]
      z = rotated[2]
    }
    const p = rotate(x, y, z, yaw, pitch)
    xs[k] = cx + p[0] * scale
    ys[k] = cy - p[1] * scale
    centroidZ += p[2]
  }
  let textX = 0
  let textY = 0
  for (let k = 0; k < points; k++) {
    textX += xs[k] / points
    textY += ys[k] / points
  }
  return { xs, ys, centroidZ: centroidZ / points, textX, textY }
}

function polygon(pts) {
  const path = new Path2D()
  path.moveTo(pts.xs[0], pts.ys[0])
  for (let k = 1; k < pts.xs.length; k++) path.lineTo(pts.xs[k], pts.ys[k])
  path.closePath()
  return path
}

// ---------- 交互 / 生命周期 ----------

function onPointerDown(e) {
  if (e.button !== 0 && e.pointerType === 'mouse') return
  dragging.value = true
  // 软件光栅下，拖拽期间用低分辨率 backing store；GPU 保持全分辨率。
  if (SOFTWARE_RASTER) {
    measure(0.5)
    requestDraw(true)
  }
  // 云层 120ms 定时重绘会在拖拽中造成周期性长帧；拖拽期间统一暂停。
  syncCloudTimer()
  dragX = e.clientX
  dragY = e.clientY
  try { canvasRef.value?.setPointerCapture?.(e.pointerId) } catch { /* 合成事件/指针已释放 */ }
  e.preventDefault()
}

function onPointerMove(e) {
  if (!dragging.value) return
  yaw += (e.clientX - dragX) * 0.01
  pitch = clamp(pitch + (e.clientY - dragY) * 0.01, -1.4, 1.4)
  dragX = e.clientX
  dragY = e.clientY
  requestDraw()
}

function onPointerUp(e) {
  if (!dragging.value) return
  dragging.value = false
  if (SOFTWARE_RASTER) {
    measure(1)
    requestDraw(true)
  }
  syncCloudTimer()
  try { canvasRef.value?.releasePointerCapture?.(e.pointerId) } catch { /* 忽略 */ }
}

function syncCloudTimer() {
  const hasClouds = !document.hidden
    && !dragging.value
    && props.tab === 1
    && !!props.scene?.appearance?.clouds?.length
    && props.scene.appearance.clouds.some((l) => l.cloudSpeed)
  if (hasClouds && !timer) {
    timer = setInterval(() => {
      cloudTime += 0.12
      requestDraw()
    }, 120)
  } else if (!hasClouds && timer) {
    clearInterval(timer)
    timer = null
  }
}

watch(() => [props.scene, props.tab, props.highlightId, props.vanillaLabel], () => {
  syncCloudTimer()
  requestDraw(true)
})

watch(() => props.tab, () => { if (props.tab !== 1) syncCloudTimer() })

onMounted(() => {
  measure()
  ro = new ResizeObserver(() => { measure(); requestDraw(true) })
  if (wrapRef.value) ro.observe(wrapRef.value)
  window.addEventListener('resize', onWindowResize)
  document.addEventListener('visibilitychange', onVisibility)
  syncCloudTimer()
  requestDraw(true)
})

function onWindowResize() {
  measure()
  requestDraw(true)
}

function onVisibility() {
  syncCloudTimer()
  if (!document.hidden) requestDraw(true)
}

onUnmounted(() => {
  ro?.disconnect()
  ro = null
  window.removeEventListener('resize', onWindowResize)
  document.removeEventListener('visibilitychange', onVisibility)
  if (timer) { clearInterval(timer); timer = null }
  if (raf) { cancelAnimationFrame(raf); raf = 0 }
  atmosphereCache = { key: '', sprite: null }
})
</script>

<template>
  <div ref="wrapRef" class="cosmos-stage">
    <canvas
      ref="canvasRef"
      class="cosmos-canvas-2d"
      :class="{ dragging }"
      role="img"
      :aria-label="label"
      @pointerdown="onPointerDown"
      @pointermove="onPointerMove"
      @pointerup="onPointerUp"
      @pointercancel="onPointerUp"
      @contextmenu.prevent />
  </div>
</template>

<style scoped>
/* 父组件 cosmos.css 的 scoped 规则只作用于本组件根节点，管不到 canvas 子节点；
   这里补上 100% 约束，否则 canvas 会按 width/height 属性像素溢出舞台并被裁切。 */
.cosmos-canvas-2d {
  display: block;
  width: 100%;
  height: 100%;
  cursor: grab;
  touch-action: none;
  outline: none;
  user-select: none;
}
.cosmos-canvas-2d.dragging { cursor: grabbing; }
</style>
