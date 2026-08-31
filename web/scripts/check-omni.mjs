#!/usr/bin/env node
// Rust ↔ TS parity for the decision-record commitment.
//
// The fixture is not a snapshot of this code's output. It is a record produced by
// `scema decide` in `scematica-omni`, carrying the digests **Rust** computed. So this
// script answers the only question that matters about the port in `lib/omni/canonical.ts`:
// does it produce the same bytes as the authoritative implementation?
//
// One differing byte and `/omni` reports an untampered record as INVALID, which is the most
// damaging possible failure — it teaches the reader to stop believing the verifier.
//
// The fixture is chosen for the case that actually broke: it contains 17-significant-digit
// floats such as -0.13286666666666663, which `serde_json` cannot round-trip bit-exactly.
// That is why both sides hash floats as fixed-point, and re-deriving this record's root is
// what pins the two scales together.
//
//   node scripts/check-omni.mjs        (Node 22+; types are stripped natively)

import { readFileSync } from 'node:fs'
import { createHash, webcrypto } from 'node:crypto'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

import { canonicalBytes, parseCanonical, toFixed, toHex } from '../lib/omni/canonical.ts'
import { verifyRecordText } from '../lib/omni/verify.ts'
import { cell, abstentionHeadline } from '../lib/omni/view.ts'
import { renderFractal, renderFractalPng, growthOf, plannedCuts } from '../lib/omni/fractal.ts'
import {
  arcPath,
  base64,
  divRound,
  fixed2,
  fmt,
  metadataFor,
  plateSourceFromText,
  renderSvg,
  truncate,
} from '../lib/omni/nft.ts'

const here = dirname(fileURLToPath(import.meta.url))
const fixturePath = join(here, '..', 'lib', 'omni', 'fixtures', 'record.json')

const sha256 = async (bytes) => new Uint8Array(createHash('sha256').update(bytes).digest())

let failures = 0
let checks = 0

function check(name, fn) {
  checks += 1
  try {
    const r = fn()
    if (r instanceof Promise) return r.then(
      () => console.log(`  ok   ${name}`),
      (e) => { failures += 1; console.error(`  FAIL ${name}\n       ${e.message}`) }
    )
    console.log(`  ok   ${name}`)
  } catch (e) {
    failures += 1
    console.error(`  FAIL ${name}\n       ${e.message}`)
  }
  return Promise.resolve()
}

function assert(cond, msg) {
  if (!cond) throw new Error(msg)
}

const text = readFileSync(fixturePath, 'utf8')
const record = JSON.parse(text)

console.log('omni — Rust↔TS commitment parity\n')

await check('the fixture is a real record with a full commitment', () => {
  for (const f of ['world', 'goal', 'hypotheses', 'projections', 'policy', 'decision', 'root']) {
    assert(typeof record.commitment[f] === 'string', `commitment.${f} missing`)
    assert(record.commitment[f].length === 64, `commitment.${f} is not a sha256 hex`)
  }
})

await check('the fixture contains floats serde_json cannot round-trip', () => {
  // If this ever stops being true the parity check still passes, but it stops covering the
  // case it was built for — so it fails loudly rather than silently weakening.
  const floats = new Set()
  const scan = (v) => {
    if (typeof v === 'number' && !Number.isInteger(v)) floats.add(v)
    else if (Array.isArray(v)) v.forEach(scan)
    else if (v && typeof v === 'object') Object.values(v).forEach(scan)
  }
  scan(record)
  const long = [...floats].filter((f) => String(f).replace(/[-.]/g, '').length >= 16)
  assert(long.length > 0, 'regenerate the fixture from a real `scema decide` run')
})

await check('TS re-derives the exact digests Rust wrote', async () => {
  const v = await verifyRecordText(text, sha256)
  assert(
    v.valid,
    `TS disagrees with Rust on: ${v.mismatches
      .map((m) => `${m.field} (rust ${m.committed.slice(0, 12)}… ts ${m.recomputed.slice(0, 12)}…)`)
      .join(', ')}`
  )
})

await check('the short id is the first 8 hex of the root', () => {
  assert(record.id === record.commitment.root.slice(0, 8), 'id does not match the root')
})

await check('WebCrypto and node agree, so the browser path is the checked path', async () => {
  const webSha = async (bytes) => new Uint8Array(await webcrypto.subtle.digest('SHA-256', bytes))
  const a = await verifyRecordText(text, sha256)
  const b = await verifyRecordText(text, webSha)
  assert(a.valid && b.valid, 'one of the two hash paths disagreed')
})

// Tampering is done by editing the record TEXT, which is both what a real edit looks like
// and the only faithful method — see the `JSON.stringify` hazard checked further down.

/**
 * Replace the first occurrence of a literal in the record text, failing if it is absent.
 *
 * The fixture is `to_string_pretty` output, so patterns carry the space after the colon and
 * the number exactly as Rust wrote it — `0.0`, not JavaScript's `0`. That difference is the
 * whole subject of the last check in this file.
 */
function edit(from, to) {
  const at = text.indexOf(from)
  assert(at !== -1, `fixture does not contain ${JSON.stringify(from)}`)
  return text.slice(0, at) + to + text.slice(at + from.length)
}

/** The first long float in the record, as it appears in the text. */
const LONG_FLOAT = (text.match(/-?\d+\.\d{14,}/) || [])[0]
assert(LONG_FLOAT, 'fixture has no high-precision float; regenerate it')

await check('editing a projected term is caught and named', async () => {
  const tampered = edit(`"value": ${LONG_FLOAT}`, '"value": 0.99')
  const v = await verifyRecordText(tampered, sha256)
  assert(!v.valid, 'a raised term must not verify')
  assert(v.mismatches.some((m) => m.field === 'projections'), 'the moved field must be named')
})

await check('editing a lambda weight moves policy and decision separately', async () => {
  const tampered = edit('"risk": 0.6', '"risk": 0.01')
  const v = await verifyRecordText(tampered, sha256)
  assert(v.mismatches.some((m) => m.field === 'policy'), 'policy must move')
  assert(v.mismatches.some((m) => m.field === 'decision'), 'decision must move')
})

await check('rewriting only the root is flagged as a hand edit', async () => {
  const tampered = edit(`"root": "${record.commitment.root}"`, `"root": "${'0'.repeat(64)}"`)
  const v = await verifyRecordText(tampered, sha256)
  assert(v.rootOnly, `every part must still verify; mismatches: ${v.mismatches.map((m) => m.field)}`)
})

await check('an edit below the bound resolution is not claimed to be caught', async () => {
  // The stated deal, checked rather than asserted in prose: the commitment binds to 1e-9,
  // and a difference smaller than that cannot move any gate in scema-policy.
  const nudged = String(Number(LONG_FLOAT) + 1e-13)
  assert(nudged !== LONG_FLOAT, 'the nudge must change the text')
  const v = await verifyRecordText(edit(`"value": ${LONG_FLOAT}`, `"value": ${nudged}`), sha256)
  assert(v.valid, `a sub-nano difference is not bound; got ${v.mismatches.map((m) => m.field)}`)
})

await check('an edit at the bound resolution is caught', async () => {
  const nudged = String(Number(LONG_FLOAT) + 1e-8)
  const v = await verifyRecordText(edit(`"value": ${LONG_FLOAT}`, `"value": ${nudged}`), sha256)
  assert(!v.valid, '1e-8 is above the 1e-9 resolution and must be bound')
})

await check('a record re-serialised by JSON.stringify does NOT verify, and that is correct', async () => {
  // The hazard that makes `verifyRecordText` take raw text rather than an object, and the
  // reason `/omni` must never round-trip a record before checking it.
  //
  // `JSON.parse` collapses Rust's `0.0` to the number 0, and `JSON.stringify` writes it back
  // as `0` — with no fraction. The canonical encoding then classifies it as an INTEGER,
  // under a different tag from the FLOAT Rust hashed, and the digest legitimately differs.
  // Nothing is wrong with the record; the round trip destroyed information the encoding
  // depends on.
  const roundTripped = JSON.stringify(JSON.parse(text))
  assert(roundTripped.includes('"value":0,'), 'expected JSON.stringify to drop a .0 somewhere')
  const v = await verifyRecordText(roundTripped, sha256)
  assert(!v.valid, 'if this ever passes, the integer/float distinction has been lost')
})

await check('the integer/float distinction survives the port', () => {
  // JavaScript has one number type. If the parser collapsed `2` and `2.0`, a record could
  // be altered without changing its digest.
  const asInt = canonicalBytes(parseCanonical('2'))
  const asFloat = canonicalBytes(parseCanonical('2.0'))
  assert(toHex(asInt) !== toHex(asFloat), '2 and 2.0 must not encode alike')
  assert(asInt[0] === 0x02 && asFloat[0] === 0x03, 'wrong tags')
})

await check('object keys are sorted by UTF-8 bytes, not by locale', () => {
  const a = canonicalBytes(parseCanonical('{"a":1,"B":2}'))
  const b = canonicalBytes(parseCanonical('{"B":2,"a":1}'))
  assert(toHex(a) === toHex(b), 'key order must not change the digest')
  // Byte order puts uppercase first; `localeCompare` would not, and Rust sorts bytes.
  // Layout: TAG_OBJECT(1) + count(8) + TAG_STR(1) + keylen(8) = 18 bytes before the first key.
  assert(
    a[18] === 'B'.charCodeAt(0),
    `uppercase B must sort before lowercase a, got ${String.fromCharCode(a[18])}`
  )
})

await check('string length is prefixed in bytes, not characters', () => {
  const bytes = canonicalBytes(parseCanonical('"é"'))
  // tag + 8 length bytes; the length must be 2 (UTF-8), not 1 (JS string length).
  assert(bytes[8] === 2, `expected a byte length of 2, got ${bytes[8]}`)
})

await check('negative zero hashes as zero', () => {
  assert(toFixed(-0) === toFixed(0), '-0.0 and 0.0 must agree')
  assert(
    toHex(canonicalBytes(parseCanonical('-0.0'))) === toHex(canonicalBytes(parseCanonical('0.0'))),
    'canonical bytes must agree'
  )
})

await check('unmeasured terms render as an em dash, never a zero', () => {
  assert(cell({ measured: false, value: 0, symbol: 'R', name: 'gain', note: '' }) === '—')
  assert(cell({ measured: true, value: 0, symbol: 'R', name: 'gain', note: '' }) === '0.00')
})

await check('every abstention reason has a headline', () => {
  const reasons = [
    { reason: 'no_candidates' },
    { reason: 'all_forbidden', count: 2 },
    { reason: 'no_positive_utility', best: -0.1 },
    { reason: 'too_little_measured', coverage: { measured: 1, total: 5 }, floor: 0.4 },
    { reason: 'contested', by: 'dqstar', utility: -0.4, note: '' },
  ]
  for (const r of reasons) {
    const h = abstentionHeadline(r)
    assert(h && h !== r.reason, `no headline for ${r.reason}`)
  }
})

// ── the world plate ───────────────────────────────────────────────────────────
//
// `lib/omni/nft.ts` is a port of the Rust crate `scema-nft`, and unlike the render rule in
// `view.ts` — where three implementations share a *rule* and each is tested separately —
// this one has to produce the **same bytes**. The plate is derived from a decision record,
// so an image that depends on which runtime drew it is not a derivative of anything: the
// CLI and the browser would mint two different tokens for one world.
//
// The fixture carries Rust's answer, exactly as `record.json` above carries digests Rust
// computed. It is regenerated by `cargo test -p scema-nft` and that test fails rather than
// silently rewriting it, so a drift shows up on whichever side changed.


/** CRC-32, written out here rather than imported: a checker that shares the implementation
 *  it is checking agrees with its own bugs. */
function crc32(data) {
  let c = 0xffffffff
  for (let i = 0; i < data.length; i += 1) {
    c = (c ^ data[i]) >>> 0
    for (let k = 0; k < 8; k += 1) c = c & 1 ? (0xedb88320 ^ (c >>> 1)) >>> 0 : c >>> 1
  }
  return (c ^ 0xffffffff) >>> 0
}

const nftDir = join(here, '..', '..', 'scematica-omni', 'crates', 'scema-nft', 'fixtures')
const plateWorld = JSON.parse(readFileSync(join(nftDir, 'parity-world.json'), 'utf8'))
const platePath = join(nftDir, 'parity-plate.svg')
const fractalPath = join(nftDir, 'parity-fractal.svg')

await check('the Rust plate and the TypeScript plate are byte-identical', () => {
  const want = readFileSync(platePath, 'utf8').replace(/\r\n/g, '\n').trimEnd()
  const digest = readFileSync(join(nftDir, 'parity-digest.txt'), 'utf8').trim()
  const got = renderSvg(plateWorld, digest)
  if (got !== want) {
    // Name the first divergence rather than dumping four kilobytes of SVG. The failures
    // this catches are almost always one number, and the offset is the fastest way to it.
    let i = 0
    while (i < Math.min(got.length, want.length) && got[i] === want[i]) i += 1
    throw new Error(
      `plates diverge at byte ${i}\n  rust: ${JSON.stringify(want.slice(i, i + 60))}\n  ts:   ${JSON.stringify(got.slice(i, i + 60))}`
    )
  }
})

await check('the geometry helpers agree with the Rust integer arithmetic', () => {
  // The four divergences that byte-parity actually turns on, pinned individually so a
  // failure names the cause rather than an offset in a 4KB string.
  assert(divRound(5, 10) === 1 && divRound(-5, 10) === -1, 'rounding must be half away from zero')
  assert(fmt(256000) === '256' && fmt(256500) === '256.5' && fmt(-500) === '-0.5', 'fmt')
  assert(fixed2(0.125) === '0.13' && fixed2(0) === '0.00', 'fixed2')
  assert(Array.from(truncate('😀😀😀😀', 3)).length === 3, 'truncate counts code points')
})

await check('a zero sweep draws nothing and a full sweep draws a circle', () => {
  // The distinction the plate turns on, at the level of the path builder. A degenerate arc
  // renders as a *full circle* on some engines, which is the picture of total coverage —
  // the worst thing a zero gauge could draw.
  assert(arcPath(100000, 0, 0) === '', 'a zero sweep must draw nothing')
  assert((arcPath(100000, 0, 360).match(/A/g) || []).length === 2, 'a full sweep is two arcs')
})

await check('an unmeasured extent draws dashed and a measured zero draws nothing', () => {
  const unbounded = { ...plateWorld, extent: { observed: 6, total: null, note: 'cap' } }
  const measuredZero = { ...plateWorld, extent: { observed: 0, total: 9, note: 'none' } }
  assert(renderSvg(unbounded, 'aa').includes('UNBOUNDED'), 'unbounded must say so')
  assert(!renderSvg(measuredZero, 'aa').includes('UNBOUNDED'), 'a measured zero is not unbounded')
  assert(renderSvg(measuredZero, 'aa').includes('EXTENT 0/9'), 'a measured zero keeps its denominator')
})

await check('an empty world and an illegible one do not draw the same picture', () => {
  // `legibility` returns 0 for both, and cannot tell them apart. The picture must.
  const empty = renderSvg({ ...plateWorld, objects: [] }, 'aa')
  const illegible = renderSvg(
    { ...plateWorld, objects: plateWorld.objects.map((o) => ({ ...o, provenance: { kind: 'absent' } })) },
    'aa'
  )
  assert(empty.includes('∅'), 'nothing-to-read must show the empty glyph')
  assert(!illegible.includes('∅'), 'a measured zero is not nothing')
  assert(illegible.includes('0.00'), 'a measured zero prints as a number')
  assert(empty !== illegible, 'the two must not be the same picture')
})

await check('a capped plate says how many it drew', () => {
  // Capping is fine; capping silently is not — a reader who counts marks would come away
  // with a wrong count. Same rule as the tail line on `render::signals_capped`.
  const many = { ...plateWorld, signals: Array.from({ length: 41 }, () => plateWorld.signals[0]) }
  const svg = renderSvg(many, 'aa')
  assert(svg.includes('COVERAGE 41/41'), 'the true denominator must survive capping')
  assert(svg.includes('· 32 DRAWN'), 'the cap must be disclosed')
  assert(!renderSvg(plateWorld, 'aa').includes('DRAWN'), 'an uncapped plate must not say DRAWN')
})

await check('base64 encodes UTF-8 bytes, matching the Rust encoder', () => {
  // The `btoa` trap: it operates on a binary string and mangles anything above U+00FF, so
  // an observer name with an accent would produce a different token here than in the CLI.
  const enc = (s) => base64(new TextEncoder().encode(s))
  assert(enc('foobar') === 'Zm9vYmFy', 'rfc4648 vector')
  assert(enc('f') === 'Zg==' && enc('fo') === 'Zm8=', 'padding')
  assert(enc('ä') === 'w6Q=' && enc('∅') === '4oiF', 'non-ascii must encode from utf-8 bytes')
})

await check('the Rust metadata and the TypeScript metadata agree', () => {
  const want = JSON.parse(readFileSync(join(nftDir, 'parity-metadata.json'), 'utf8'))
  const digest = readFileSync(join(nftDir, 'parity-digest.txt'), 'utf8').trim()
  // The fractal, not the plate: `scema nft` embeds the growth now, so the metadata's
  // `image` data URI is built from it. Using the plate here would pass on attributes and
  // fail on the image, which is exactly what it did when this was first wired.
  const svg = renderFractal(plateWorld, digest)
  const got = metadataFor(plateWorld, svg, digest)
  assert(
    JSON.stringify(got.attributes) === JSON.stringify(want.attributes),
    `attributes differ:\n  rust: ${JSON.stringify(want.attributes)}\n  ts:   ${JSON.stringify(got.attributes)}`
  )
  assert(got.image === want.image, 'the inlined image data URI must match')
  assert(got.description === want.description, 'the description must match')
})

await check('token metadata carries no score, rank or rarity', () => {
  // Guarding an absence, because the pressure to add one is permanent and whoever adds it
  // will be doing something that looks helpful. A rank computed from these numbers would be
  // a value of exactly the right shape with nothing behind it.
  const digest = 'a'.repeat(64)
  const text = JSON.stringify(metadataFor(plateWorld, '<svg/>', digest)).toLowerCase()
  for (const banned of ['rarity', '"score"', '"rank"', '"tier"', '"level"']) {
    assert(!text.includes(banned), `metadata must not contain ${banned}`)
  }
})

await check('an unmeasured legibility is a glyph in the traits, never a zero', () => {
  const m = metadataFor({ ...plateWorld, objects: [] }, '<svg/>', 'aa')
  const leg = m.attributes.find((a) => a.trait_type === 'Legibility')
  assert(leg.value === '∅', `legibility of an unobserved world must not be a number, got ${leg.value}`)
})

await check('the plate never depends on the clock', () => {
  // No "minted at". A timestamp taken at render time would make every regeneration a
  // different token, which defeats deriving the image from the record at all.
  const digest = 'a'.repeat(64)
  const text = JSON.stringify(metadataFor(plateWorld, renderSvg(plateWorld, digest), digest))
  for (const banned of ['minted', 'generated_at', 'rendered_at', 'createdAt']) {
    assert(!text.includes(banned), `metadata must not contain ${banned}`)
  }
})

await check('a record contributes its stored commitment, not a recomputed one', async () => {
  // An edited record must produce a plate whose commitment does not match its own world —
  // that mismatch is the tamper signal. Recomputing here would paper over it.
  const text = JSON.stringify({ world: plateWorld, commitment: { world: '0000stored0000' } })
  const src = await plateSourceFromText(text, sha256)
  assert(src.kind === 'record', 'must be recognised as a record')
  assert(src.digest === '0000stored0000', 'the stored commitment must be used verbatim')
})

await check('a bare world is committed with the canonical encoding', async () => {
  const text = readFileSync(join(nftDir, 'parity-world.json'), 'utf8')
  const want = readFileSync(join(nftDir, 'parity-digest.txt'), 'utf8').trim()
  const src = await plateSourceFromText(text, sha256)
  assert(src.kind === 'world', 'must be recognised as a world')
  assert(
    src.digest === want,
    `world digest disagrees with Rust\n  rust: ${want}\n  ts:   ${src.digest}`
  )
})

await check('anything else is refused rather than drawn', async () => {
  let threw = false
  try {
    await plateSourceFromText('{"hello":1}', sha256)
  } catch (e) {
    threw = true
    assert(e.message.includes('WorldState'), 'the error should name both accepted shapes')
  }
  assert(threw, 'a document that is neither must be refused')
})

await check('the Rust fractal and the TypeScript fractal are byte-identical', () => {
  // Harder than the plate: a recursion amplifies any disagreement, so a one-ULP difference
  // at the root is a visibly different tree by the fourth level. That is why there is no
  // float arithmetic in the growth and the RNG is 32-bit.
  const want = readFileSync(fractalPath, 'utf8').replace(/\r\n/g, '\n').trimEnd()
  const digest = readFileSync(join(nftDir, 'parity-digest.txt'), 'utf8').trim()
  const got = renderFractal(plateWorld, digest)
  if (got !== want) {
    let i = 0
    while (i < Math.min(got.length, want.length) && got[i] === want[i]) i += 1
    throw new Error(
      `fractals diverge at byte ${i}\n  rust: ${JSON.stringify(want.slice(i, i + 80))}\n  ts:   ${JSON.stringify(got.slice(i, i + 80))}`
    )
  }
})

await check('the Rust PNG and the TypeScript PNG are byte-identical', () => {
  // The reason a rasteriser exists here at all. Handing the SVG to a canvas would antialias
  // differently in every browser, and an image that depends on which runtime drew it is not
  // a derivative of the record — it is two artefacts sharing a name. So the whole path is
  // ported: supersample, integer downsample, hand-rolled zlib, hand-rolled PNG.
  //
  // This also stands in for a font-table test. A glyph differing by one bit changes these
  // bytes, so the 95-entry table in `raster.ts` is checked by the picture rather than by a
  // second copy of itself.
  const want = readFileSync(join(nftDir, 'parity-fractal.png'))
  const digest = readFileSync(join(nftDir, 'parity-digest.txt'), 'utf8').trim()
  const got = renderFractalPng(plateWorld, digest, 256)

  if (got.length !== want.length) {
    throw new Error(`png lengths differ: rust ${want.length}, ts ${got.length}`)
  }
  let i = 0
  while (i < want.length && got[i] === want[i]) i += 1
  if (i < want.length) {
    // Naming *where* matters: a divergence in the first 8 bytes is the signature, in the
    // next 25 the header, and anywhere after that a pixel — three completely different bugs.
    const where = i < 8 ? 'signature' : i < 33 ? 'IHDR' : 'pixel data'
    throw new Error(
      `pngs diverge at byte ${i} (${where}): rust 0x${want[i].toString(16)}, ts 0x${got[i].toString(16)}`
    )
  }
})

await check('the PNG is a real PNG and its chunk CRCs check out', () => {
  // The encoder is hand-written, so "the two agree" is not enough on its own — they could
  // agree on something no decoder will open. This reads the container back.
  const png = renderFractalPng(plateWorld, 'deadbeef', 64)
  const sig = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]
  sig.forEach((b, i) => assert(png[i] === b, `signature byte ${i}`))

  const view = new DataView(png.buffer, png.byteOffset, png.byteLength)
  const kinds = []
  let off = 8
  while (off < png.length) {
    const len = view.getUint32(off)
    const kind = String.fromCharCode(...png.slice(off + 4, off + 8))
    const body = png.slice(off + 4, off + 8 + len)
    const stated = view.getUint32(off + 8 + len)
    assert(crc32(body) === stated, `${kind} chunk CRC`)
    kinds.push(kind)
    off += 12 + len
  }
  assert(off === png.length, 'chunks must tile the file exactly')
  assert(kinds.join(',') === 'IHDR,IDAT,IEND', `chunks were ${kinds.join(',')}`)

  assert(view.getUint32(16) === 64 && view.getUint32(20) === 64, 'IHDR dimensions')
  assert(png[24] === 8 && png[25] === 2, 'must be 8-bit RGB')
})

await check('the zlib stream is stored blocks, so the bytes depend only on the pixels', () => {
  // A compressor's heuristics are not part of any specification. Two implementations both
  // emitting valid deflate would still emit *different* deflate, and byte-parity would be
  // unreachable. Stored blocks make the output a pure function of the pixels.
  const png = renderFractalPng(plateWorld, 'deadbeef', 64)
  const view = new DataView(png.buffer, png.byteOffset, png.byteLength)
  const len = view.getUint32(33)
  const idat = png.slice(41, 41 + len)
  assert(idat[0] === 0x78 && idat[1] === 0x01, 'zlib header must be 78 01')
  // BTYPE lives in bits 1-2 of the first byte of a block; 00 is "stored".
  assert(((idat[2] >> 1) & 0x03) === 0, 'the first deflate block must be stored')
  // Raw size: one filter byte per row plus three bytes per pixel, uncompressed.
  const raw = 64 * (1 + 64 * 3)
  const blocks = Math.ceil(raw / 65535)
  assert(len === 2 + blocks * 5 + raw + 4, `IDAT length ${len} is not stored-block sized`)
})

await check('an unmeasured signal rasterises differently from a measured one', () => {
  // The hollow-versus-filled distinction is the em-dash rule in raster form, and it has to
  // survive supersampling and downsampling — antialiasing a ring hard enough turns it into a
  // disc, which would draw an estimate as a count.
  const digest = readFileSync(join(nftDir, 'parity-digest.txt'), 'utf8').trim()
  const counted = { ...plateWorld, signals: plateWorld.signals.map((x) => ({ ...x, measured: true })) }
  const guessed = { ...plateWorld, signals: plateWorld.signals.map((x) => ({ ...x, measured: false })) }
  const a = renderFractalPng(counted, digest, 128)
  const b = renderFractalPng(guessed, digest, 128)
  assert(a.length === b.length, 'same geometry, so same size')
  assert(!a.every((v, i) => v === b[i]), 'counted and estimated marks must not raster alike')
})

await check('the growth parameters are read from the world, not invented', () => {
  const g = growthOf(plateWorld)
  assert(g.depth >= 3 && g.depth <= 9, `depth ${g.depth}`)
  assert(g.arity >= 2, `arity ${g.arity}`)
  // Six observed of nine, three blind spots, three counted signals of five.
  assert(g.cuts === plateWorld.blind_spots.length, 'cuts must equal blind spots')
})

await check('blind spots cut exactly as many limbs as were reported', () => {
  // A per-node probability compounds down the recursion: three blind spots cut twenty-six
  // limbs in the first version. That is the form claiming more ignorance than the observer
  // did, which is the same class of error as rendering an unmeasured term as 0.00.
  for (const n of [1, 2, 3, 5]) {
    const w = { ...plateWorld, blind_spots: Array.from({ length: n }, (_, i) => `spot ${i}`) }
    const svg = renderFractal(w, 'deadbeef')
    const cut = (svg.match(/stroke="#6f6690"/g) || []).length
    assert(cut === n, `${n} blind spot(s) produced ${cut} cut(s)`)
    assert(svg.includes(`${n} LIMB(S) CUT`), `footer must report ${n}`)
  }
})

await check('cutting never annihilates the form', () => {
  // Three blind spots fit exactly on level one of an arity-3 tree, and cutting all three
  // deletes the canopy — reporting "three of six unreadable" as "nothing was observed".
  for (const n of [1, 3, 6, 9]) {
    const w = { ...plateWorld, blind_spots: Array.from({ length: n }, (_, i) => `spot ${i}`) }
    const svg = renderFractal(w, 'deadbeef')
    const grown = (svg.match(/stroke="#a96bff"/g) || []).length
    assert(grown > 20, `${n} blind spots left only ${grown} live branches`)
    const [, capped] = plannedCuts(growthOf(w))
    if (capped) assert(svg.includes('(CAPPED)'), 'a capped cut count must say so')
  }
})

await check('the same world grows the same form and a different digest does not', () => {
  assert(renderFractal(plateWorld, 'aaaaaaaa') === renderFractal(plateWorld, 'aaaaaaaa'))
  assert(renderFractal(plateWorld, 'aaaaaaaa') !== renderFractal(plateWorld, 'bbbbbbbb'))
})

await check('a zero seed does not collapse the rng', () => {
  // xorshift has a fixed point at zero and would return zero forever, quietly collapsing
  // every world onto one form.
  const a = renderFractal(plateWorld, '00000000')
  const b = renderFractal(plateWorld, '00000001')
  assert(a !== b, 'a zero seed must not be a special case')
})

console.log(`\n${checks - failures}/${checks} checks passed`)
if (failures > 0) process.exit(1)
