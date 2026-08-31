/**
 * A PNG of the growth, in the browser. A port of `scema_nft::raster`.
 *
 * Rust is authoritative and this must produce the **same bytes**. That is the whole reason a
 * rasteriser exists here at all: handing the SVG to a canvas would antialias differently from
 * Rust, and an image that depends on which runtime made it is not a derivative of the record
 * — it is two artefacts with one name. Porting is the only way the browser can offer a PNG
 * without breaking that.
 *
 * The port is mechanical because the original has no floats in it:
 *
 * - antialiasing is a 3× supersample and an integer box downsample with a stated rounding
 *   rule, not analytic coverage;
 * - the zlib stream is **stored** deflate blocks, so the bytes are a pure function of the
 *   pixels rather than of a compressor's heuristics;
 * - `>>> 0` after every step in `crc32` and `adler32`, because JavaScript's bitwise operators
 *   work on signed int32 where Rust's are on `u32`.
 *
 * `check:omni` compares this file's output against a PNG Rust wrote. One differing byte fails.
 */

import { cosMicro, sinMicro } from './nft.ts'

/** Supersampling factor. Must match `SS` in `raster.rs`. */
const SS = 3

const GLYPH_W = 5
const GLYPH_H = 7

export type Anchor = 'start' | 'middle' | 'end'

export type Prim =
  | {
      kind: 'line'
      a: [number, number]
      b: [number, number]
      widthMu: number
      rgb: string
      dashed: boolean
    }
  | { kind: 'disc'; c: [number, number]; rMu: number; rgb: string; filled: boolean }
  | {
      kind: 'tri'
      a: [number, number]
      b: [number, number]
      c: [number, number]
      rgb: string
      filled: boolean
    }
  | { kind: 'text'; at: [number, number]; size: number; rgb: string; anchor: Anchor; body: string }

/**
 * Printable ASCII, 0x20 to 0x7E. One byte per column, bit 0 the top row.
 *
 * Duplicated from `raster.rs` rather than generated, and deliberately given no parity test of
 * its own — the PNG comparison covers it transitively. A glyph differing by one bit produces
 * a different image, which is exactly what that check looks at.
 */
const FONT: number[][] = [
  [0x00, 0x00, 0x00, 0x00, 0x00], // space
  [0x00, 0x00, 0x5f, 0x00, 0x00], // !
  [0x00, 0x07, 0x00, 0x07, 0x00], // "
  [0x14, 0x7f, 0x14, 0x7f, 0x14], // #
  [0x24, 0x2a, 0x7f, 0x2a, 0x12], // $
  [0x23, 0x13, 0x08, 0x64, 0x62], // %
  [0x36, 0x49, 0x55, 0x22, 0x50], // &
  [0x00, 0x05, 0x03, 0x00, 0x00], // '
  [0x00, 0x1c, 0x22, 0x41, 0x00], // (
  [0x00, 0x41, 0x22, 0x1c, 0x00], // )
  [0x14, 0x08, 0x3e, 0x08, 0x14], // *
  [0x08, 0x08, 0x3e, 0x08, 0x08], // +
  [0x00, 0x50, 0x30, 0x00, 0x00], // ,
  [0x08, 0x08, 0x08, 0x08, 0x08], // -
  [0x00, 0x60, 0x60, 0x00, 0x00], // .
  [0x20, 0x10, 0x08, 0x04, 0x02], // /
  [0x3e, 0x51, 0x49, 0x45, 0x3e], // 0
  [0x00, 0x42, 0x7f, 0x40, 0x00], // 1
  [0x42, 0x61, 0x51, 0x49, 0x46], // 2
  [0x21, 0x41, 0x45, 0x4b, 0x31], // 3
  [0x18, 0x14, 0x12, 0x7f, 0x10], // 4
  [0x27, 0x45, 0x45, 0x45, 0x39], // 5
  [0x3c, 0x4a, 0x49, 0x49, 0x30], // 6
  [0x01, 0x71, 0x09, 0x05, 0x03], // 7
  [0x36, 0x49, 0x49, 0x49, 0x36], // 8
  [0x06, 0x49, 0x49, 0x29, 0x1e], // 9
  [0x00, 0x36, 0x36, 0x00, 0x00], // :
  [0x00, 0x56, 0x36, 0x00, 0x00], // ;
  [0x08, 0x14, 0x22, 0x41, 0x00], // <
  [0x14, 0x14, 0x14, 0x14, 0x14], // =
  [0x00, 0x41, 0x22, 0x14, 0x08], // >
  [0x02, 0x01, 0x51, 0x09, 0x06], // ?
  [0x32, 0x49, 0x79, 0x41, 0x3e], // @
  [0x7e, 0x11, 0x11, 0x11, 0x7e], // A
  [0x7f, 0x49, 0x49, 0x49, 0x36], // B
  [0x3e, 0x41, 0x41, 0x41, 0x22], // C
  [0x7f, 0x41, 0x41, 0x22, 0x1c], // D
  [0x7f, 0x49, 0x49, 0x49, 0x41], // E
  [0x7f, 0x09, 0x09, 0x09, 0x01], // F
  [0x3e, 0x41, 0x49, 0x49, 0x7a], // G
  [0x7f, 0x08, 0x08, 0x08, 0x7f], // H
  [0x00, 0x41, 0x7f, 0x41, 0x00], // I
  [0x20, 0x40, 0x41, 0x3f, 0x01], // J
  [0x7f, 0x08, 0x14, 0x22, 0x41], // K
  [0x7f, 0x40, 0x40, 0x40, 0x40], // L
  [0x7f, 0x02, 0x0c, 0x02, 0x7f], // M
  [0x7f, 0x04, 0x08, 0x10, 0x7f], // N
  [0x3e, 0x41, 0x41, 0x41, 0x3e], // O
  [0x7f, 0x09, 0x09, 0x09, 0x06], // P
  [0x3e, 0x41, 0x51, 0x21, 0x5e], // Q
  [0x7f, 0x09, 0x19, 0x29, 0x46], // R
  [0x46, 0x49, 0x49, 0x49, 0x31], // S
  [0x01, 0x01, 0x7f, 0x01, 0x01], // T
  [0x3f, 0x40, 0x40, 0x40, 0x3f], // U
  [0x1f, 0x20, 0x40, 0x20, 0x1f], // V
  [0x3f, 0x40, 0x38, 0x40, 0x3f], // W
  [0x63, 0x14, 0x08, 0x14, 0x63], // X
  [0x07, 0x08, 0x70, 0x08, 0x07], // Y
  [0x61, 0x51, 0x49, 0x45, 0x43], // Z
  [0x00, 0x7f, 0x41, 0x41, 0x00], // [
  [0x02, 0x04, 0x08, 0x10, 0x20], // backslash
  [0x00, 0x41, 0x41, 0x7f, 0x00], // ]
  [0x04, 0x02, 0x01, 0x02, 0x04], // ^
  [0x40, 0x40, 0x40, 0x40, 0x40], // _
  [0x00, 0x01, 0x02, 0x04, 0x00], // `
  [0x20, 0x54, 0x54, 0x54, 0x78], // a
  [0x7f, 0x48, 0x44, 0x44, 0x38], // b
  [0x38, 0x44, 0x44, 0x44, 0x20], // c
  [0x38, 0x44, 0x44, 0x48, 0x7f], // d
  [0x38, 0x54, 0x54, 0x54, 0x18], // e
  [0x08, 0x7e, 0x09, 0x01, 0x02], // f
  [0x0c, 0x52, 0x52, 0x52, 0x3e], // g
  [0x7f, 0x08, 0x04, 0x04, 0x78], // h
  [0x00, 0x44, 0x7d, 0x40, 0x00], // i
  [0x20, 0x40, 0x44, 0x3d, 0x00], // j
  [0x7f, 0x10, 0x28, 0x44, 0x00], // k
  [0x00, 0x41, 0x7f, 0x40, 0x00], // l
  [0x7c, 0x04, 0x18, 0x04, 0x78], // m
  [0x7c, 0x08, 0x04, 0x04, 0x78], // n
  [0x38, 0x44, 0x44, 0x44, 0x38], // o
  [0x7c, 0x14, 0x14, 0x14, 0x08], // p
  [0x08, 0x14, 0x14, 0x18, 0x7c], // q
  [0x7c, 0x08, 0x04, 0x04, 0x08], // r
  [0x48, 0x54, 0x54, 0x54, 0x20], // s
  [0x04, 0x3f, 0x44, 0x40, 0x20], // t
  [0x3c, 0x40, 0x40, 0x20, 0x7c], // u
  [0x1c, 0x20, 0x40, 0x20, 0x1c], // v
  [0x3c, 0x40, 0x30, 0x40, 0x3c], // w
  [0x44, 0x28, 0x10, 0x28, 0x44], // x
  [0x0c, 0x50, 0x50, 0x50, 0x3c], // y
  [0x44, 0x64, 0x54, 0x4c, 0x44], // z
  [0x00, 0x08, 0x36, 0x41, 0x00], // {
  [0x00, 0x00, 0x7f, 0x00, 0x00], // |
  [0x00, 0x41, 0x36, 0x08, 0x00], // }
  [0x08, 0x08, 0x2a, 0x1c, 0x08], // ~
]

/**
 * The bitmap for one character.
 *
 * The non-ASCII glyphs the legend uses get shapes of their own — `∅` especially, which is how
 * an unmeasured coverage is written everywhere else here. A fallback box would turn a
 * specific statement into a missing-character marker.
 */
function glyph(c: string): number[] {
  switch (c) {
    case '·':
      return [0x00, 0x00, 0x08, 0x00, 0x00]
    case '°':
      return [0x00, 0x06, 0x09, 0x06, 0x00]
    case '∅':
      return [0x3e, 0x51, 0x49, 0x45, 0x3e]
    case '—':
    case '–':
      return [0x08, 0x08, 0x08, 0x08, 0x08]
    case '≥':
      return [0x44, 0x54, 0x54, 0x54, 0x28]
    default: {
      const i = c.codePointAt(0) ?? 0
      if (i >= 0x20 && i <= 0x7e) return FONT[i - 0x20]
      // Visible box, never a blank: a silently dropped glyph makes a label read as
      // something it is not.
      return [0x7f, 0x41, 0x41, 0x41, 0x7f]
    }
  }
}

type Rgb = [number, number, number]

function hexToRgb(hex: string): Rgb {
  return [
    parseInt(hex.slice(1, 3), 16),
    parseInt(hex.slice(3, 5), 16),
    parseInt(hex.slice(5, 7), 16),
  ]
}

class Buf {
  readonly w: number
  readonly h: number
  private px: Uint8Array

  constructor(w: number, h: number, bg: Rgb) {
    this.w = w
    this.h = h
    this.px = new Uint8Array(w * h * 3)
    for (let i = 0; i < w * h; i += 1) {
      this.px[i * 3] = bg[0]
      this.px[i * 3 + 1] = bg[1]
      this.px[i * 3 + 2] = bg[2]
    }
  }

  set(x: number, y: number, rgb: Rgb): void {
    if (x < 0 || y < 0 || x >= this.w || y >= this.h) return
    const i = (y * this.w + x) * 3
    this.px[i] = rgb[0]
    this.px[i + 1] = rgb[1]
    this.px[i + 2] = rgb[2]
  }

  /** A filled disc — the brush for every stroke, which gives round joins and caps. */
  disc(cx: number, cy: number, r: number, rgb: Rgb): void {
    if (r <= 0) {
      this.set(cx, cy, rgb)
      return
    }
    const rr = r * r
    for (let dy = -r; dy <= r; dy += 1) {
      for (let dx = -r; dx <= r; dx += 1) {
        if (dx * dx + dy * dy <= rr) this.set(cx + dx, cy + dy, rgb)
      }
    }
  }

  line(x0: number, y0: number, x1: number, y1: number, r: number, rgb: Rgb, dashed: boolean): void {
    const dx = Math.abs(x1 - x0)
    const dy = -Math.abs(y1 - y0)
    const sx = x0 < x1 ? 1 : -1
    const sy = y0 < y1 ? 1 : -1
    let err = dx + dy
    let x = x0
    let y = y0
    let step = 0
    for (;;) {
      const on = !dashed || Math.trunc(step / (3 * SS)) % 2 === 0
      if (on) this.disc(x, y, r, rgb)
      if (x === x1 && y === y1) break
      const e2 = 2 * err
      if (e2 >= dy) {
        err += dy
        x += sx
      }
      if (e2 <= dx) {
        err += dx
        y += sy
      }
      step += 1
      if (step > 20000) break
    }
  }

  tri(a: [number, number], b: [number, number], c: [number, number], rgb: Rgb): void {
    const minx = Math.min(a[0], b[0], c[0])
    const maxx = Math.max(a[0], b[0], c[0])
    const miny = Math.min(a[1], b[1], c[1])
    const maxy = Math.max(a[1], b[1], c[1])
    const cross = (p: [number, number], q: [number, number], r: [number, number]) =>
      (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    for (let y = miny; y <= maxy; y += 1) {
      for (let x = minx; x <= maxx; x += 1) {
        const p: [number, number] = [x, y]
        const d1 = cross(a, b, p)
        const d2 = cross(b, c, p)
        const d3 = cross(c, a, p)
        const neg = d1 < 0 || d2 < 0 || d3 < 0
        const pos = d1 > 0 || d2 > 0 || d3 > 0
        if (!(neg && pos)) this.set(x, y, rgb)
      }
    }
  }

  text(x: number, y: number, scale: number, anchor: Anchor, body: string, rgb: Rgb): void {
    const chars = Array.from(body)
    const advance = (GLYPH_W + 1) * scale
    const width = chars.length * advance
    const start =
      anchor === 'start' ? x : anchor === 'middle' ? x - Math.trunc(width / 2) : x - width
    // `y` is the SVG baseline; the font draws from its top, so lift it.
    const top = y - GLYPH_H * scale
    for (let i = 0; i < chars.length; i += 1) {
      const g = glyph(chars[i])
      const gx = start + i * advance
      for (let col = 0; col < GLYPH_W; col += 1) {
        for (let row = 0; row < GLYPH_H; row += 1) {
          if (g[col] & (1 << row)) {
            for (let sy = 0; sy < scale; sy += 1) {
              for (let sx = 0; sx < scale; sx += 1) {
                this.set(gx + col * scale + sx, top + row * scale + sy, rgb)
              }
            }
          }
        }
      }
    }
  }

  /** Average each SS×SS block. Integer division, round half up, stated not inherited. */
  downsample(outW: number, outH: number): Uint8Array {
    const n = SS * SS
    const half = Math.trunc(n / 2)
    const out = new Uint8Array(outW * outH * 3)
    let o = 0
    for (let y = 0; y < outH; y += 1) {
      for (let x = 0; x < outW; x += 1) {
        let r = 0
        let g = 0
        let b = 0
        for (let sy = 0; sy < SS; sy += 1) {
          for (let sx = 0; sx < SS; sx += 1) {
            const i = ((y * SS + sy) * this.w + (x * SS + sx)) * 3
            r += this.px[i]
            g += this.px[i + 1]
            b += this.px[i + 2]
          }
        }
        out[o] = Math.trunc((r + half) / n)
        out[o + 1] = Math.trunc((g + half) / n)
        out[o + 2] = Math.trunc((b + half) / n)
        o += 3
      }
    }
    return out
  }
}

// ── png ───────────────────────────────────────────────────────────────────────

const CRC_TABLE = (() => {
  const t = new Uint32Array(256)
  for (let i = 0; i < 256; i += 1) {
    let c = i
    for (let k = 0; k < 8; k += 1) c = c & 1 ? (0xedb88320 ^ (c >>> 1)) >>> 0 : c >>> 1
    t[i] = c >>> 0
  }
  return t
})()

export function crc32(data: Uint8Array): number {
  let c = 0xffffffff
  for (let i = 0; i < data.length; i += 1) c = (CRC_TABLE[(c ^ data[i]) & 0xff] ^ (c >>> 8)) >>> 0
  return (c ^ 0xffffffff) >>> 0
}

export function adler32(data: Uint8Array): number {
  let a = 1
  let b = 0
  for (let i = 0; i < data.length; i += 1) {
    a = (a + data[i]) % 65521
    b = (b + a) % 65521
  }
  return ((b << 16) | a) >>> 0
}

function be32(n: number): number[] {
  return [(n >>> 24) & 0xff, (n >>> 16) & 0xff, (n >>> 8) & 0xff, n & 0xff]
}

function chunk(out: number[], kind: string, body: Uint8Array | number[]): void {
  const b = body instanceof Uint8Array ? body : Uint8Array.from(body)
  for (const v of be32(b.length)) out.push(v)
  const k = Array.from(kind, (ch) => ch.charCodeAt(0))
  for (const v of k) out.push(v)
  for (let i = 0; i < b.length; i += 1) out.push(b[i])
  const crcInput = new Uint8Array(4 + b.length)
  crcInput.set(k, 0)
  crcInput.set(b, 4)
  for (const v of be32(crc32(crcInput))) out.push(v)
}

/** A zlib stream of stored deflate blocks. See the module note for why. */
function zlibStored(data: Uint8Array): Uint8Array {
  const out: number[] = [0x78, 0x01]
  let i = 0
  if (data.length === 0) out.push(0x01, 0x00, 0x00, 0xff, 0xff)
  while (i < data.length) {
    const n = Math.min(data.length - i, 65535)
    out.push(i + n >= data.length ? 1 : 0)
    out.push(n & 0xff, (n >>> 8) & 0xff)
    const inv = ~n & 0xffff
    out.push(inv & 0xff, (inv >>> 8) & 0xff)
    for (let k = 0; k < n; k += 1) out.push(data[i + k])
    i += n
  }
  for (const v of be32(adler32(data))) out.push(v)
  return Uint8Array.from(out)
}

function encodePng(w: number, h: number, rgb: Uint8Array): Uint8Array<ArrayBuffer> {
  // Filter type 0 on every row: any other filter is a compression aid, and there is no
  // compression here to aid.
  const raw = new Uint8Array(h * (1 + w * 3))
  for (let y = 0; y < h; y += 1) {
    raw[y * (1 + w * 3)] = 0
    raw.set(rgb.subarray(y * w * 3, (y + 1) * w * 3), y * (1 + w * 3) + 1)
  }
  const out: number[] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]
  chunk(out, 'IHDR', [...be32(w), ...be32(h), 8, 2, 0, 0, 0])
  chunk(out, 'IDAT', zlibStored(raw))
  chunk(out, 'IEND', [])
  return Uint8Array.from(out)
}

/** Rasterise a scene to a PNG. `size` is the output edge; `view` the scene's unit space. */
export function renderPng(
  prims: Prim[],
  view: number,
  size: number,
  bgHex: string,
): Uint8Array<ArrayBuffer> {
  const big = size * SS
  const buf = new Buf(big, big, hexToRgb(bgHex))

  const toPx = (v: number) => Math.trunc((v * big) / (view * 1000))
  const scaleMu = (v: number) => Math.trunc((v * big) / (view * 1000))

  for (const p of prims) {
    if (p.kind === 'line') {
      const r = Math.max(1, Math.trunc(scaleMu(p.widthMu) / 2))
      buf.line(toPx(p.a[0]), toPx(p.a[1]), toPx(p.b[0]), toPx(p.b[1]), r, hexToRgb(p.rgb), p.dashed)
    } else if (p.kind === 'disc') {
      const r = Math.max(1, scaleMu(p.rMu))
      const rgb = hexToRgb(p.rgb)
      if (p.filled) {
        buf.disc(toPx(p.c[0]), toPx(p.c[1]), r, rgb)
      } else {
        // A hollow mark is a ring: an estimated magnitude must not draw as a counted one,
        // and that distinction has to survive rasterisation.
        for (let t = 0; t < Math.max(1, SS); t += 1) {
          const rr = Math.max(1, r - t)
          const steps = Math.max(16, rr * 8)
          for (let i = 0; i < steps; i += 1) {
            const deg = Math.trunc((i * 360) / steps)
            const x = toPx(p.c[0]) + Math.trunc((rr * cosMicro(deg)) / 1000000)
            const y = toPx(p.c[1]) + Math.trunc((rr * sinMicro(deg)) / 1000000)
            buf.set(x, y, rgb)
          }
        }
      }
    } else if (p.kind === 'tri') {
      const pa: [number, number] = [toPx(p.a[0]), toPx(p.a[1])]
      const pb: [number, number] = [toPx(p.b[0]), toPx(p.b[1])]
      const pc: [number, number] = [toPx(p.c[0]), toPx(p.c[1])]
      const rgb = hexToRgb(p.rgb)
      if (p.filled) {
        buf.tri(pa, pb, pc, rgb)
      } else {
        const r = Math.max(1, Math.trunc(SS / 2))
        buf.line(pa[0], pa[1], pb[0], pb[1], r, rgb, false)
        buf.line(pb[0], pb[1], pc[0], pc[1], r, rgb, false)
        buf.line(pc[0], pc[1], pa[0], pa[1], r, rgb, false)
      }
    } else {
      const px = Math.trunc((p.size * big) / view)
      const s = Math.max(1, Math.trunc(px / GLYPH_H))
      buf.text(toPx(p.at[0]), toPx(p.at[1]), s, p.anchor, p.body, hexToRgb(p.rgb))
    }
  }

  return encodePng(size, size, buf.downsample(size, size))
}
