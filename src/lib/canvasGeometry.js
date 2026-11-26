
export const CONTENT = { WIDTH: 192, HEADER: 32, ROW: 32, PORT_R: 4 }
export const CARRIER = { WIDTH: 260, HEIGHT: 64 }

export function cardSize(card) {
  if (card?.carrier) return { w: CARRIER.WIDTH, h: CARRIER.HEIGHT }
  const rows = card?.rows?.length || 0
  return { w: CONTENT.WIDTH, h: CONTENT.HEADER + rows * CONTENT.ROW }
}

export function portAnchor(card, fieldKey) {
  if (card?.carrier) {
    return fieldKey == null ? { x: CARRIER.WIDTH - 8 + CONTENT.PORT_R, y: CARRIER.HEIGHT / 2 } : null
  }
  if (fieldKey == null) return { x: CONTENT.WIDTH - CONTENT.PORT_R, y: CONTENT.HEADER / 2 }
  const index = (card.rows || []).findIndex((row) => row.name === fieldKey)
  if (index < 0 || !(card.inputFields || []).includes(fieldKey)) return null
  return { x: 1 + CONTENT.PORT_R, y: CONTENT.HEADER + index * CONTENT.ROW + CONTENT.ROW / 2 }
}

const FALLBACK_Y = 150 / 2

export function portWorld(node, card, fieldKey) {
  if (!node || !card) return { x: 0, y: 0 }
  const anchor = portAnchor(card, fieldKey)
  if (anchor) return { x: node.x + anchor.x, y: node.y + anchor.y }
  return fieldKey == null
    ? { x: node.x + CONTENT.WIDTH, y: node.y + FALLBACK_Y }
    : { x: node.x, y: node.y + FALLBACK_Y }
}


export function viaPoints(reroutes) {
  const out = []
  for (const point of reroutes || []) {
    if (Array.isArray(point) && point.length >= 2) out.push([point[0], point[1]])
  }
  return out
}

export function bezierSegment(x1, y1, x2, y2) {
  const midX = x1 + (x2 - x1) * 0.5
  return [x1, y1, midX, y1, midX, y2, x2, y2]
}

export function straightPoints(x1, y1, x2, y2, reroutes) {
  const points = [[x1, y1]]
  const add = (x, y) => {
    const last = points[points.length - 1]
    if (!(last[0] === x && last[1] === y)) points.push([x, y])
  }
  for (const point of viaPoints(reroutes)) add(point[0], point[1])
  add(x2, y2)
  return points
}

export function orthogonalPoints(x1, y1, x2, y2, reroutes) {
  const points = [[x1, y1]]
  const add = (x, y) => {
    const last = points[points.length - 1]
    if (!(last[0] === x && last[1] === y)) points.push([x, y])
  }
  const bend = (ax, ay, bx, by) => {
    if (ax === bx || ay === by) return
    const midX = ax + (bx - ax) * 0.5
    add(midX, ay)
    add(midX, by)
  }
  let prevX = x1
  let prevY = y1
  for (const point of viaPoints(reroutes)) {
    bend(prevX, prevY, point[0], point[1])
    add(point[0], point[1])
    prevX = point[0]
    prevY = point[1]
  }
  bend(prevX, prevY, x2, y2)
  add(x2, y2)
  return points
}

export function arrowAngle(style, x1, y1, x2, y2, reroutes) {
  const via = viaPoints(reroutes)
  if (style === 'bezier') {
    let prevX = x1
    for (const point of via) prevX = point[0]
    const midX = prevX + (x2 - prevX) * 0.5
    return Math.atan2(0, x2 - midX)
  }
  const points = style === 'orthogonal'
    ? orthogonalPoints(x1, y1, x2, y2, reroutes)
    : straightPoints(x1, y1, x2, y2, reroutes)
  if (points.length >= 2) {
    const before = points[points.length - 2]
    return Math.atan2(y2 - before[1], x2 - before[0])
  }
  return Math.atan2(y2 - y1, x2 - x1)
}

export function hitReroute(worldX, worldY, reroutes, radius) {
  let best = null
  let bestDistance = radius
  viaPoints(reroutes).forEach((point, i) => {
    const distance = Math.hypot(point[0] - worldX, point[1] - worldY)
    if (distance <= bestDistance) {
      bestDistance = distance
      best = i
    }
  })
  return best
}

export function edgeGeometry(edge, style, start, end) {
  const via = viaPoints(edge.reroutes)
  let d
  if (style === 'bezier') {
    let prevX = start.x
    let prevY = start.y
    d = `M ${start.x} ${start.y}`
    for (const point of via) {
      const seg = bezierSegment(prevX, prevY, point[0], point[1])
      d += ` C ${seg[2]} ${seg[3]}, ${seg[4]} ${seg[5]}, ${seg[6]} ${seg[7]}`
      prevX = point[0]
      prevY = point[1]
    }
    const seg = bezierSegment(prevX, prevY, end.x, end.y)
    d += ` C ${seg[2]} ${seg[3]}, ${seg[4]} ${seg[5]}, ${seg[6]} ${seg[7]}`
  } else {
    const points = style === 'orthogonal'
      ? orthogonalPoints(start.x, start.y, end.x, end.y, edge.reroutes)
      : straightPoints(start.x, start.y, end.x, end.y, edge.reroutes)
    d = points.map((point, i) => `${i === 0 ? 'M' : 'L'} ${point[0]} ${point[1]}`).join(' ')
  }
  const angle = arrowAngle(style, start.x, start.y, end.x, end.y, edge.reroutes)
  const tipX = end.x - Math.cos(angle) * 7
  const tipY = end.y - Math.sin(angle) * 7
  return {
    d,
    arrow: { x: end.x, y: end.y, angleDeg: (angle * 180) / Math.PI },
    label: { x: (start.x + end.x) / 2 - 20, y: (start.y + end.y) / 2 - 9 },
    tip: { x: tipX, y: tipY },
  }
}
