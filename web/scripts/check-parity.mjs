#!/usr/bin/env node
// Pin the TS pool scorer against the cases pool_scorer.rs asserts in its unit tests.
//
// Two implementations of the same scoring edge will drift. This is the tripwire: it runs
// the same fixtures the Rust tests use, so a change to the likelihood ratios or the
// sigmoid on either side fails here instead of silently mis-scoring live pools.
//
// Hermetic on purpose — no network, no feed. The live-feed adapter is exercised by the
// app itself; this checks only the maths.
//
//   node scripts/check-parity.mjs        (Node 22+; types are stripped natively)

import { scorePool, __fixtures } from '../lib/feed/scorer.ts'

let failed = 0

for (const f of __fixtures) {
  const score = scorePool(f.input)
  const ok = f.expect(score)
  if (!ok) failed++
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${f.name.padEnd(34)} score=${score.toFixed(2)}`)
}

// Monotonicity guards. These are not in the Rust tests but follow from its ladders, and
// they catch a whole class of copy errors (an inverted comparison, a transposed rung)
// that the point fixtures above can miss.
const invariants = [
  {
    name: 'fresher pool scores higher',
    ok: () => scorePool({ sizeSol: 30, ageSecs: 5 }) > scorePool({ sizeSol: 30, ageSecs: 120 }),
  },
  {
    name: 'sweet-spot size beats micro-cap',
    ok: () => scorePool({ sizeSol: 30, ageSecs: 10 }) > scorePool({ sizeSol: 4, ageSecs: 10 }),
  },
  {
    name: 'sweet-spot size beats whale pool',
    ok: () => scorePool({ sizeSol: 30, ageSecs: 10 }) > scorePool({ sizeSol: 400, ageSecs: 10 }),
  },
  {
    name: 'scores stay within 0..100',
    ok: () => [0, 0.5, 3, 12, 30, 90, 200, 5000].every(sizeSol => {
      const s = scorePool({ sizeSol, ageSecs: 10 })
      return s >= 0 && s <= 100
    }),
  },
]

for (const inv of invariants) {
  const ok = inv.ok()
  if (!ok) failed++
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${inv.name}`)
}

console.log(failed === 0 ? '\nparity OK' : `\n${failed} parity check(s) FAILED`)
process.exit(failed === 0 ? 0 : 1)
