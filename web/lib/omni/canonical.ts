/**
 * Canonical encoding — a port of `scematica-omni/crates/scema-verify/src/canonical.rs`.
 *
 * **Rust is authoritative.** This file exists so a decision record can be verified in a
 * browser without shipping the daemon, and it is only useful if it produces the same bytes.
 * One differing byte and every verdict on `/omni` is wrong in the most damaging possible
 * direction: an untampered record reported as INVALID, which teaches the reader to stop
 * believing the verifier.
 *
 * `npm run check:omni` is what catches drift. It re-derives the commitment of a real record
 * produced by `scema decide` and compares against the digests Rust wrote into it — so the
 * fixture is not a snapshot of this code's output, it is Rust's answer.
 *
 * ## The encoding
 *
 * | Type | Bytes |
 * |---|---|
 * | null | `00` |
 * | bool | `01` + `00`/`01` |
 * | integer | `02` + i64 big-endian |
 * | float | `03` + `round(v * 1e9)` as i64 big-endian |
 * | string | `04` + u64 BE byte length + UTF-8 |
 * | array | `05` + u64 BE count + items in order |
 * | object | `06` + u64 BE count + (key, value) pairs sorted by key |
 * | float (out of range / non-finite) | `07` + normalised IEEE-754 bits, big-endian |
 *
 * ## Two things that are easy to get wrong in a port
 *
 * 1. **Integer versus float is decided by the JSON text, not the JavaScript value.**
 *    JavaScript has one number type, so `2` and `2.0` are indistinguishable once parsed —
 *    but Rust encodes the first under `02` and the second under `03`, and they must not
 *    collide. So this module never consumes `JSON.parse` output directly; it consumes a
 *    tree built by {@link parseCanonical}, which records whether each number was written
 *    with a fraction or an exponent.
 * 2. **String length is in bytes, not characters.** Rust prefixes `s.len()`, which is the
 *    UTF-8 byte count. `"é".length` is 1 in JavaScript and 2 in Rust.
 */

/** A JSON value that remembers whether each number was integral in the source text. */
export type CanonValue =
  | { t: 'null' }
  | { t: 'bool'; v: boolean }
  | { t: 'int'; v: bigint }
  | { t: 'float'; v: number }
  | { t: 'string'; v: string }
  | { t: 'array'; v: CanonValue[] }
  | { t: 'object'; v: Array<[string, CanonValue]> }

const TAG_NULL = 0x00
const TAG_BOOL = 0x01
const TAG_INT = 0x02
const TAG_FLOAT = 0x03
const TAG_STR = 0x04
const TAG_ARRAY = 0x05
const TAG_OBJECT = 0x06
const TAG_FLOAT_BITS = 0x07

/** Fixed-point scale for hashed floats. Must equal `FIXED_SCALE` in Rust. */
export const FIXED_SCALE = 1_000_000_000

/**
 * Parse JSON into a tree that preserves the integer/float distinction.
 *
 * `JSON.parse`'s reviver receives the already-coerced number, so it cannot help. This is a
 * small hand-rolled parser instead — it only has to handle what `serde_json` emits.
 */
export function parseCanonical(text: string): CanonValue {
  let i = 0

  function ws() {
    while (i < text.length && (text[i] === ' ' || text[i] === '\n' || text[i] === '\r' || text[i] === '\t')) i += 1
  }

  function fail(msg: string): never {
    throw new SyntaxError(`${msg} at offset ${i}`)
  }

  function value(): CanonValue {
    ws()
    const c = text[i]
    if (c === '{') return object()
    if (c === '[') return array()
    if (c === '"') return { t: 'string', v: str() }
    if (c === 't') {
      expect('true')
      return { t: 'bool', v: true }
    }
    if (c === 'f') {
      expect('false')
      return { t: 'bool', v: false }
    }
    if (c === 'n') {
      expect('null')
      return { t: 'null' }
    }
    return number()
  }

  function expect(word: string) {
    if (text.slice(i, i + word.length) !== word) fail(`expected ${word}`)
    i += word.length
  }

  function object(): CanonValue {
    i += 1 // {
    const entries: Array<[string, CanonValue]> = []
    ws()
    if (text[i] === '}') {
      i += 1
      return { t: 'object', v: entries }
    }
    for (;;) {
      ws()
      if (text[i] !== '"') fail('expected a key')
      const k = str()
      ws()
      if (text[i] !== ':') fail('expected :')
      i += 1
      entries.push([k, value()])
      ws()
      if (text[i] === ',') {
        i += 1
        continue
      }
      if (text[i] === '}') {
        i += 1
        return { t: 'object', v: entries }
      }
      fail('expected , or }')
    }
  }

  function array(): CanonValue {
    i += 1 // [
    const items: CanonValue[] = []
    ws()
    if (text[i] === ']') {
      i += 1
      return { t: 'array', v: items }
    }
    for (;;) {
      items.push(value())
      ws()
      if (text[i] === ',') {
        i += 1
        continue
      }
      if (text[i] === ']') {
        i += 1
        return { t: 'array', v: items }
      }
      fail('expected , or ]')
    }
  }

  function str(): string {
    i += 1 // opening quote
    let out = ''
    for (;;) {
      const c = text[i]
      if (c === undefined) fail('unterminated string')
      if (c === '"') {
        i += 1
        return out
      }
      if (c === '\\') {
        i += 1
        const e = text[i]
        i += 1
        switch (e) {
          case '"': out += '"'; break
          case '\\': out += '\\'; break
          case '/': out += '/'; break
          case 'b': out += '\b'; break
          case 'f': out += '\f'; break
          case 'n': out += '\n'; break
          case 'r': out += '\r'; break
          case 't': out += '\t'; break
          case 'u': {
            out += String.fromCharCode(parseInt(text.slice(i, i + 4), 16))
            i += 4
            break
          }
          default: fail(`bad escape \\${e}`)
        }
        continue
      }
      out += c
      i += 1
    }
  }

  function number(): CanonValue {
    const start = i
    if (text[i] === '-') i += 1
    while (i < text.length && text[i] >= '0' && text[i] <= '9') i += 1
    let fractional = false
    if (text[i] === '.') {
      fractional = true
      i += 1
      while (i < text.length && text[i] >= '0' && text[i] <= '9') i += 1
    }
    if (text[i] === 'e' || text[i] === 'E') {
      fractional = true
      i += 1
      if (text[i] === '+' || text[i] === '-') i += 1
      while (i < text.length && text[i] >= '0' && text[i] <= '9') i += 1
    }
    const raw = text.slice(start, i)
    if (raw === '' || raw === '-') fail('expected a number')
    // A number written without a fraction or exponent is an integer, exactly as
    // `serde_json` classifies it. `BigInt` so a u64 beyond 2^53 survives — record
    // timestamps are i64 and counts are usize.
    return fractional ? { t: 'float', v: Number(raw) } : { t: 'int', v: BigInt(raw) }
  }

  const root = value()
  ws()
  if (i !== text.length) fail('trailing content')
  return root
}

function pushU64(out: number[], n: bigint) {
  for (let shift = 56n; shift >= 0n; shift -= 8n) {
    out.push(Number((n >> shift) & 0xffn))
  }
}

function pushI64(out: number[], n: bigint) {
  // Two's complement over 64 bits, matching Rust's `i64::to_be_bytes`.
  pushU64(out, BigInt.asUintN(64, n))
}

/** Normalise a float to a single bit pattern per mathematical value. */
function normalisedBits(f: number): bigint {
  const buf = new DataView(new ArrayBuffer(8))
  if (Number.isNaN(f)) {
    buf.setFloat64(0, NaN)
  } else if (f === 0) {
    buf.setFloat64(0, 0)
  } else {
    buf.setFloat64(0, f)
  }
  return buf.getBigUint64(0)
}

const I64_MIN = -(2n ** 63n)
const I64_MAX = 2n ** 63n - 1n

/** Quantise a float for hashing, or `null` when it does not fit. */
export function toFixed(f: number): bigint | null {
  if (!Number.isFinite(f)) return null
  // `Math.round` and Rust's `f64::round` agree except on halfway cases, where Rust rounds
  // away from zero and `Math.round` rounds toward +Infinity. Handled explicitly: a term of
  // exactly -0.0000000005 must not hash differently on the two sides.
  const scaled = f * FIXED_SCALE
  const rounded = scaled < 0 ? -Math.round(-scaled) : Math.round(scaled)
  if (!Number.isFinite(rounded)) return null
  const asBig = BigInt(Math.trunc(rounded))
  return asBig >= I64_MIN && asBig <= I64_MAX ? asBig : null
}

function encodeInto(v: CanonValue, out: number[]) {
  switch (v.t) {
    case 'null':
      out.push(TAG_NULL)
      break
    case 'bool':
      out.push(TAG_BOOL, v.v ? 1 : 0)
      break
    case 'int':
      out.push(TAG_INT)
      pushI64(out, v.v)
      break
    case 'float': {
      const fixed = toFixed(v.v)
      if (fixed === null) {
        out.push(TAG_FLOAT_BITS)
        pushU64(out, normalisedBits(v.v))
      } else {
        out.push(TAG_FLOAT)
        pushI64(out, fixed)
      }
      break
    }
    case 'string': {
      out.push(TAG_STR)
      const bytes = new TextEncoder().encode(v.v)
      // Byte length, not `String.length`. Rust prefixes `s.len()`.
      pushU64(out, BigInt(bytes.length))
      for (const b of bytes) out.push(b)
      break
    }
    case 'array':
      out.push(TAG_ARRAY)
      pushU64(out, BigInt(v.v.length))
      for (const item of v.v) encodeInto(item, out)
      break
    case 'object': {
      out.push(TAG_OBJECT)
      pushU64(out, BigInt(v.v.length))
      // Sorted by key. Rust sorts `&String`, which is a byte-wise comparison, so the sort
      // here compares UTF-8 bytes rather than using `localeCompare` — which is
      // locale-dependent and would reorder keys on some machines.
      const sorted = [...v.v].sort((a, b) => compareBytes(a[0], b[0]))
      for (const [k, item] of sorted) {
        out.push(TAG_STR)
        const kb = new TextEncoder().encode(k)
        pushU64(out, BigInt(kb.length))
        for (const b of kb) out.push(b)
        encodeInto(item, out)
      }
      break
    }
  }
}

function compareBytes(a: string, b: string): number {
  const ab = new TextEncoder().encode(a)
  const bb = new TextEncoder().encode(b)
  const n = Math.min(ab.length, bb.length)
  for (let i = 0; i < n; i += 1) {
    if (ab[i] !== bb[i]) return ab[i] - bb[i]
  }
  return ab.length - bb.length
}

/** Canonical bytes for a parsed value. */
export function canonicalBytes(v: CanonValue): Uint8Array {
  const out: number[] = []
  encodeInto(v, out)
  return Uint8Array.from(out)
}

/** Canonical bytes for a sub-tree of a record, addressed by top-level key. */
export function fieldBytes(root: CanonValue, key: string): Uint8Array | null {
  if (root.t !== 'object') return null
  const found = root.v.find(([k]) => k === key)
  return found ? canonicalBytes(found[1]) : null
}

/** Bytes for `digest_of_digests`: length-prefixed label, then the 32 raw digest bytes. */
export function rootBytes(parts: Array<[string, Uint8Array]>): Uint8Array {
  const out: number[] = []
  for (const [label, digest] of parts) {
    const lb = new TextEncoder().encode(label)
    pushU64(out, BigInt(lb.length))
    for (const b of lb) out.push(b)
    for (const b of digest) out.push(b)
  }
  return Uint8Array.from(out)
}

export function toHex(bytes: Uint8Array): string {
  return Array.from(bytes)
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('')
}

export function fromHex(hex: string): Uint8Array {
  const clean = hex.trim().toLowerCase()
  const out = new Uint8Array(clean.length / 2)
  for (let i = 0; i < out.length; i += 1) {
    out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16)
  }
  return out
}
