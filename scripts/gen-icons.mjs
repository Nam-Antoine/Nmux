/**
 * Generates placeholder app + tray icons (a ">_" prompt glyph) as PNGs with
 * no dependencies. Replace resources/icon.png with real artwork whenever.
 *
 *   pnpm icons
 */
import { deflateSync } from 'node:zlib'
import { mkdirSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const outDir = join(dirname(fileURLToPath(import.meta.url)), '..', 'resources')

const BG = [27, 31, 42]
const ACCENT = [232, 115, 74]

/** Colour of the icon at normalised coordinates (u, v) in [0, 1]. Alpha 0 = outside. */
function sample(u, v) {
  // Rounded square background.
  const r = 0.22
  const cx = Math.min(Math.max(u, r), 1 - r)
  const cy = Math.min(Math.max(v, r), 1 - r)
  if (Math.hypot(u - cx, v - cy) > r) return [0, 0, 0, 0]

  // Chevron ">" : two thick strokes.
  const stroke = 0.075
  const upper = distToSegment(u, v, 0.28, 0.32, 0.5, 0.5) < stroke
  const lower = distToSegment(u, v, 0.28, 0.68, 0.5, 0.5) < stroke
  // Underscore "_".
  const bar = u >= 0.56 && u <= 0.76 && v >= 0.63 && v <= 0.72
  return upper || lower || bar ? [...ACCENT, 255] : [...BG, 255]
}

function distToSegment(px, py, ax, ay, bx, by) {
  const dx = bx - ax
  const dy = by - ay
  const t = Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
  return Math.hypot(px - (ax + t * dx), py - (ay + t * dy))
}

function render(size, supersample = 4) {
  const rgba = Buffer.alloc(size * size * 4)
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      let r = 0, g = 0, b = 0, a = 0
      for (let sy = 0; sy < supersample; sy++) {
        for (let sx = 0; sx < supersample; sx++) {
          const u = (x + (sx + 0.5) / supersample) / size
          const v = (y + (sy + 0.5) / supersample) / size
          const [cr, cg, cb, ca] = sample(u, v)
          r += cr * ca
          g += cg * ca
          b += cb * ca
          a += ca
        }
      }
      const i = (y * size + x) * 4
      if (a > 0) {
        rgba[i] = Math.round(r / a)
        rgba[i + 1] = Math.round(g / a)
        rgba[i + 2] = Math.round(b / a)
        rgba[i + 3] = Math.round(a / (supersample * supersample))
      }
    }
  }
  return rgba
}

// ---- minimal PNG encoder ----------------------------------------------------

const CRC_TABLE = new Int32Array(256).map((_, n) => {
  let c = n
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
  return c
})

function crc32(buf) {
  let c = -1
  for (const byte of buf) c = CRC_TABLE[(c ^ byte) & 0xff] ^ (c >>> 8)
  return (c ^ -1) >>> 0
}

function chunk(type, data) {
  const len = Buffer.alloc(4)
  len.writeUInt32BE(data.length)
  const body = Buffer.concat([Buffer.from(type, 'ascii'), data])
  const crc = Buffer.alloc(4)
  crc.writeUInt32BE(crc32(body))
  return Buffer.concat([len, body, crc])
}

function encodePng(size, rgba) {
  const stride = size * 4
  const raw = Buffer.alloc((stride + 1) * size)
  for (let y = 0; y < size; y++) {
    raw[y * (stride + 1)] = 0 // filter: none
    rgba.copy(raw, y * (stride + 1) + 1, y * stride, (y + 1) * stride)
  }
  const ihdr = Buffer.alloc(13)
  ihdr.writeUInt32BE(size, 0)
  ihdr.writeUInt32BE(size, 4)
  ihdr[8] = 8 // bit depth
  ihdr[9] = 6 // colour type RGBA
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0))
  ])
}

mkdirSync(outDir, { recursive: true })
for (const [name, size] of [
  ['icon.png', 256],
  ['tray.png', 32]
]) {
  writeFileSync(join(outDir, name), encodePng(size, render(size)))
  console.log(`wrote resources/${name} (${size}x${size})`)
}
