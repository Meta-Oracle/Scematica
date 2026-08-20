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

console.log(`\n${checks - failures}/${checks} checks passed`)
if (failures > 0) process.exit(1)
