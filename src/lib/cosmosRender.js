// 星球预览渲染数学（对齐 Java ui/PlanetPreview.java 与 assets/shaders/atmosphere.frag）。
// 纯函数 + 一个离屏大气光栅器；不依赖 Vue，便于单测/复用。

export const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v))
export const clamp01 = (v) => clamp(v, 0, 1)

export function normalize(x, y, z) {
  const len = Math.sqrt(x * x + y * y + z * z) || 1
  return [x / len, y / len, z / len]
}

/** PlanetPreview.rotate：先绕 Y(yaw) 再绕 X(pitch)。 */
export function rotate(x, y, z, yaw, pitch) {
  const cy = Math.cos(yaw)
  const sy = Math.sin(yaw)
  const cp = Math.cos(pitch)
  const sp = Math.sin(pitch)
  const x1 = x * cy + z * sy
  const z1 = -x * sy + z * cy
  const y2 = y * cp - z1 * sp
  const z2 = y * sp + z1 * cp
  return [x1, y2, z2]
}

export function rotateY(x, y, z, angle) {
  const c = Math.cos(angle)
  const s = Math.sin(angle)
  return [x * c + z * s, y, -x * s + z * c]
}

/**
 * 面法线（旋转后空间）。corners 是未旋转的扁平 x,y,z 序列。
 * 与 Java PlanetPreview.faceNormal 一致：用前三个角点叉积，再按远离球心翻转。
 */
export function faceNormal(corners, yaw, pitch) {
  const p0 = rotate(corners[0], corners[1], corners[2], yaw, pitch)
  const p1 = rotate(corners[3], corners[4], corners[5], yaw, pitch)
  const p2 = rotate(corners[6], corners[7], corners[8], yaw, pitch)
  const ax = p1[0] - p0[0]
  const ay = p1[1] - p0[1]
  const az = p1[2] - p0[2]
  const bx = p2[0] - p0[0]
  const by = p2[1] - p0[1]
  const bz = p2[2] - p0[2]
  let nx = ay * bz - az * by
  let ny = az * bx - ax * bz
  let nz = ax * by - ay * bx
  const len = Math.sqrt(nx * nx + ny * ny + nz * nz)
  if (len < 1e-9) return [0, 0, 1]
  nx /= len
  ny /= len
  nz /= len
  if (nx * p0[0] + ny * p0[1] + nz * p0[2] < 0) {
    nx = -nx
    ny = -ny
    nz = -nz
  }
  return [nx, ny, nz]
}

/** Java PlanetPreview.lambert：面法线与光照方向点积，翻转朝外，按三点质心判定。 */
export function lambert(corners, light, yaw, pitch) {
  const p0 = rotate(corners[0], corners[1], corners[2], yaw, pitch)
  const p1 = rotate(corners[3], corners[4], corners[5], yaw, pitch)
  const p2 = rotate(corners[6], corners[7], corners[8], yaw, pitch)
  const ax = p1[0] - p0[0]
  const ay = p1[1] - p0[1]
  const az = p1[2] - p0[2]
  const bx = p2[0] - p0[0]
  const by = p2[1] - p0[1]
  const bz = p2[2] - p0[2]
  let nx = ay * bz - az * by
  let ny = az * bx - ax * bz
  let nz = ax * by - ay * bx
  const mx = (p0[0] + p1[0] + p2[0]) / 3
  const my = (p0[1] + p1[1] + p2[1]) / 3
  const mz = (p0[2] + p1[2] + p2[2]) / 3
  if (nx * mx + ny * my + nz * mz < 0) {
    nx = -nx
    ny = -ny
    nz = -nz
  }
  const len = Math.sqrt(nx * nx + ny * ny + nz * nz)
  if (len < 1e-9) return 0.5
  return (nx * light[0] + ny * light[1] + nz * light[2]) / len
}

/** 光照方向：Java lightFromSystem，恒星在中心，非恒星按 0.6 + index*2π/5 排布。 */
export function lightFromSystem(bodies) {
  let index = 0
  for (const body of bodies || []) {
    if (body.star) {
      index++
      continue
    }
    const angle = 0.6 + index * (Math.PI * 2) / 5
    index++
    if (body.selected) return normalize(-Math.cos(angle), -Math.sin(angle), 0)
  }
  return normalize(0.75, 0.35, 0.55)
}

/** Java PlanetPreview.maxLayerLength：角点最大模长（球面起伏可能超过 radius）。 */
export function maxLayerLength(layer) {
  let max = 1
  for (const corners of layer?.tiles || []) {
    const c = corners.corners
    for (let k = 0; k < c.length; k += 3) {
      const len = Math.sqrt(c[k] * c[k] + c[k + 1] * c[k + 1] + c[k + 2] * c[k + 2])
      if (len > max) max = len
    }
  }
  return max
}

/** '#rgb' / '#rrggbb' → [r,g,b]（0..1）；解析失败用默认大气蓝。 */
export function parseHexColor(hex, fallback = [0.3, 0.7, 1.0]) {
  const s = String(hex || '').replace(/^#/, '')
  if (/^[0-9a-fA-F]{3}$/.test(s)) {
    return [parseInt(s[0] + s[0], 16) / 255, parseInt(s[1] + s[1], 16) / 255, parseInt(s[2] + s[2], 16) / 255]
  }
  if (/^[0-9a-fA-F]{6}$/.test(s)) {
    return [parseInt(s.slice(0, 2), 16) / 255, parseInt(s.slice(2, 4), 16) / 255, parseInt(s.slice(4, 6), 16) / 255]
  }
  return fallback
}

export const rgba = (r, g, b, a = 1) =>
  `rgba(${Math.round(clamp01(r) * 255)},${Math.round(clamp01(g) * 255)},${Math.round(clamp01(b) * 255)},${clamp01(a)})`

export const hexColor = (hex, fallback = '#5f666e') => {
  const s = String(hex || '').replace(/^#/, '')
  return /^[0-9a-fA-F]{6}$/.test(s) ? '#' + s : fallback
}


// ---------- 确定性星空（Java java.util.Random(42) 48-bit LCG，逐位复刻） ----------

function javaRandom(seed) {
  const mask = (1n << 48n) - 1n
  let state = (BigInt(seed) ^ 0x5deece66dn) & mask
  const next = (bits) => {
    state = (state * 0x5deece66dn + 0xbn) & mask
    return Number(state >> BigInt(48 - bits))
  }
  return () => ((next(26) << 27) + next(27)) / 9007199254740992
}

export const STARS = (() => {
  const rand = javaRandom(42)
  return Array.from({ length: 110 }, () => ({
    x: rand(),
    y: rand(),
    alpha: 0.25 + rand() * 0.75,
  }))
})()

export function drawStars(ctx, w, h, stars = STARS) {
  ctx.save()
  ctx.fillStyle = '#ffffff'
  for (const star of stars) {
    ctx.globalAlpha = star.alpha * 0.8
    ctx.fillRect(star.x * w, star.y * h, 1.4, 1.4)
  }
  ctx.restore()
}


// ---------- 大气散射（atmosphere.frag 逐像素移植，离屏缓存） ----------

const PEAK = 0.1
const FLARE = 0.0025
const INTENSITY = 14.3
const G_M = -0.85

function rayIntersection(px, py, pz, dir, radius) {
  const b = px * dir[0] + py * dir[1] + pz * dir[2]
  const c = px * px + py * py + pz * pz - radius * radius
  let d = b * b - c
  if (d < 0) return [10000, -10000]
  d = Math.sqrt(d)
  return [-b - d, -b + d]
}

function optic(px, py, pz, qx, qy, qz, inner, outer) {
  const sx = (qx - px) / 3
  const sy = (qy - py) / 3
  const sz = (qz - pz) / 3
  let vx = px + sx * 0.5
  let vy = py + sy * 0.5
  let vz = pz + sz * 0.5
  let sum = 0
  for (let i = 0; i < 3; i++) {
    const len = Math.sqrt(vx * vx + vy * vy + vz * vz)
    sum += Math.exp(-(len - inner) * (4 / (outer - inner)))
    vx += sx
    vy += sy
    vz += sz
  }
  const stepLen = Math.sqrt(sx * sx + sy * sy + sz * sz)
  return sum * stepLen * (1 / (outer - inner))
}

/**
 * 生成大气层像素（加色混合用）。返回 { canvas, size }；无有效像素时为透明。
 * 与 Java drawAtmosphere 完全同参：inner=地形轮廓、outer=inner+radOut、eyeZ=outer*4、3+3 步。
 */
export function buildAtmosphereSprite(colorHex, radOut, inner, scale, light) {
  const color = parseHexColor(colorHex)
  const outer = inner + Math.max(0.0001, radOut)
  const eyeZ = outer * 4
  const size = Math.ceil(outer * scale) * 2 + 2
  const canvas = document.createElement('canvas')
  canvas.width = size
  canvas.height = size
  const ctx = canvas.getContext('2d')
  const image = ctx.createImageData(size, size)
  const data = image.data
  const half = size / 2
  const lx = light[0]
  const ly = light[1]
  const lz = light[2]
  const span = outer - inner
  for (let py = 0; py < size; py++) {
    const sy = (half + 0.5 - py) / scale
    for (let px = 0; px < size; px++) {
      const sx = (px - half + 0.5) / scale
      const dir = normalize(sx, sy, -eyeZ)
      const e = rayIntersection(0, 0, eyeZ, dir, outer)
      const f = rayIntersection(0, 0, eyeZ, dir, inner)
      const tEnd = Math.min(e[1], f[0])
      if (e[1] <= e[0] || tEnd <= e[0]) continue
      const len = (tEnd - e[0]) / 3
      const stepX = dir[0] * len
      const stepY = dir[1] * len
      const stepZ = dir[2] * len
      // p：进入大气壳层的点（march 起点），Java 用它计算 optic(p, v)。
      const px0 = dir[0] * e[0]
      const py0 = dir[1] * e[0]
      const pz0 = eyeZ + dir[2] * e[0]
      let vx = px0 + stepX * 0.5
      let vy = py0 + stepY * 0.5
      let vz = pz0 + stepZ * 0.5
      let sumR = 0
      let sumG = 0
      let sumB = 0
      for (let i = 0; i < 3; i++) {
        const fv = rayIntersection(vx, vy, vz, light, outer)
        const ux = vx + lx * fv[1]
        const uy = vy + ly * fv[1]
        const uz = vz + lz * fv[1]
        const n = (optic(px0, py0, pz0, vx, vy, vz, inner, outer)
          + optic(vx, vy, vz, ux, uy, uz, inner, outer)) * (Math.PI * 4)
        const dens = Math.exp(-(Math.sqrt(vx * vx + vy * vy + vz * vz) - inner) * (4 / span))
        sumR += dens * Math.exp(-n * (PEAK * color[0] + FLARE))
        sumG += dens * Math.exp(-n * (PEAK * color[1] + FLARE))
        sumB += dens * Math.exp(-n * (PEAK * color[2] + FLARE))
        vx += stepX
        vy += stepY
        vz += stepZ
      }
      sumR *= len / span
      sumG *= len / span
      sumB *= len / span
      const c = -(dir[0] * lx + dir[1] * ly + dir[2] * lz)
      const cc = c * c
      const rayleigh = 0.75 * (1 + cc)
      const gg = G_M * G_M
      const mieA = (1 - gg) * (1 + cc)
      let mieB = 1 + gg - 2 * G_M * c
      mieB *= Math.sqrt(mieB)
      mieB *= 2 + gg
      const mie = 1.5 * mieA / mieB
      const i = (py * size + px) * 4
      data[i] = Math.round(clamp01(sumR * (PEAK * color[0] * rayleigh + FLARE * mie) * INTENSITY) * 255)
      data[i + 1] = Math.round(clamp01(sumG * (PEAK * color[1] * rayleigh + FLARE * mie) * INTENSITY) * 255)
      data[i + 2] = Math.round(clamp01(sumB * (PEAK * color[2] * rayleigh + FLARE * mie) * INTENSITY) * 255)
      data[i + 3] = 255
    }
  }
  ctx.putImageData(image, 0, 0)
  return { canvas, size }
}
