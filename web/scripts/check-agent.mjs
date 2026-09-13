#!/usr/bin/env node
// Pin `/omni-agent`'s replay of the agent's proposal log.
//
// `lib/agent/replay.ts` is a PORT of the state machine in `omni-agent/src/lib/queue.ts`,
// and the TypeScript there is authoritative. A drifted port does not merely render less —
// it renders a history that did not happen, which looks like evidence. So every case here
// either replays a real event sequence and asserts the derived state, or asserts that
// something does *not* come out.
//
// The three rules being defended, each paid for elsewhere in this repository:
//
//   * An unscored draft is not a draft scored zero. `—`, never `0.00`.
//   * An unknown event is counted, not dropped. A log from a newer agent must render as
//     incomplete rather than as confidently wrong.
//   * Counts, never an invented rate. There is no approval percentage anywhere.
//
//   node --experimental-strip-types scripts/check-agent.mjs

import { existsSync, readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

import {
  finalText,
  isScored,
  labelTotals,
  labelsFor,
  replayQueue,
  scoreCell,
} from '../lib/agent/replay.ts'

let checks = 0
let failures = 0

/**
 * Run one case.
 *
 * `body` returns true, or throws. A case that returns anything else is treated as a
 * failure rather than as a pass: a harness that accepts a truthy value passes every test
 * whose body is an arrow function somebody forgot to call, which is exactly the bug this
 * file shipped with for about four minutes.
 */
const check = (name, body) => {
  checks++
  let ok = false
  let why = ''
  try {
    ok = body() === true
    if (!ok) why = 'the case did not return true'
  } catch (error) {
    why = error instanceof Error ? error.message : String(error)
  }
  if (!ok) failures++
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}${ok ? '' : ` — ${why}`}`)
}
const assert = (ok, why) => {
  if (!ok) throw new Error(why)
  return true
}

const HERE = dirname(fileURLToPath(import.meta.url))

const SCORES = { salience: 0.6, taste: 0.71, resonance: 0.4, novelty: 0.9, priority: 0.55 }

const proposed = (id, over = {}) =>
  JSON.stringify({
    type: 'proposed',
    at: '2026-09-01T10:00:00.000Z',
    proposal: {
      id,
      createdAt: `2026-09-01T10:0${id.slice(-1)}:00.000Z`,
      text: `draft ${id}`,
      rationale: 'because',
      sources: ['https://x.com/x/status/1'],
      topic: 'Solana MEV and transaction landing',
      scores: SCORES,
      status: 'pending',
      ...over,
    },
  })

const log = (...lines) => lines.join('\n') + '\n'

console.log('── replay ───────────────────────────────────────────────')

check('an empty file is no events, not an empty queue', () => {
  const r = replayQueue('')
  return r.events === 0 && r.proposals.length === 0
})

check('a proposal starts pending', () => {
  const r = replayQueue(log(proposed('a1')))
  assert(r.proposals.length === 1, 'proposal missing')
  return r.proposals[0].status === 'pending' && r.counts.pending === 1
})

check('approve then post lands on posted', () => {
  const r = replayQueue(
    log(
      proposed('a1'),
      JSON.stringify({ type: 'decided', at: 'T1', id: 'a1', status: 'approved', by: 'deadsg' }),
      JSON.stringify({ type: 'posted', at: 'T2', id: 'a1', postedId: '19999' }),
    ),
  )
  const p = r.proposals[0]
  return p.status === 'posted' && p.postedId === '19999' && p.decidedBy === 'deadsg'
})

check('a rejection is recorded, not dropped', () => {
  const r = replayQueue(
    log(
      proposed('a1'),
      JSON.stringify({ type: 'decided', at: 'T1', id: 'a1', status: 'rejected', by: 'deadsg' }),
    ),
  )
  return r.counts.rejected === 1 && r.proposals[0].status === 'rejected'
})

check('an edit replaces the published text but keeps the original', () => {
  const r = replayQueue(
    log(
      proposed('a1'),
      JSON.stringify({
        type: 'decided',
        at: 'T1',
        id: 'a1',
        status: 'approved',
        by: 'deadsg',
        editedText: 'the rewrite',
      }),
    ),
  )
  const p = r.proposals[0]
  // Both halves must survive: the difference between them is the training signal.
  return finalText(p) === 'the rewrite' && p.text === 'draft a1'
})

check('engagement attaches as a measurement', () => {
  const r = replayQueue(
    log(
      proposed('a1'),
      JSON.stringify({ type: 'posted', at: 'T2', id: 'a1', postedId: '1' }),
      JSON.stringify({ type: 'engagement', at: 'T3', id: 'a1', likes: 7, reposts: 2, replies: 1 }),
    ),
  )
  return r.proposals[0].engagement?.likes === 7
})

console.log('\n── nothing is invented ──────────────────────────────────')

check('a draft with no scores is unscored, not scored zero', () => {
  const r = replayQueue(log(proposed('a1', { scores: undefined })))
  const p = r.proposals[0]
  assert(!isScored(p), 'an absent score object became a score')
  // The em-dash rule, in the one place a reader would act on it.
  return scoreCell(p, 'taste') === '—'
})

check('a partial score object is unscored rather than zero-filled', () => {
  // Five confident numbers out of two real ones is the exact failure. All or nothing.
  const r = replayQueue(log(proposed('a1', { scores: { taste: 0.9, salience: 0.4 } })))
  return !isScored(r.proposals[0]) && scoreCell(r.proposals[0], 'novelty') === '—'
})

check('a measured zero still prints 0.00', () => {
  const r = replayQueue(log(proposed('a1', { scores: { ...SCORES, taste: 0 } })))
  // A real observation of zero is not the same as no observation, and must not hide.
  return scoreCell(r.proposals[0], 'taste') === '0.00'
})

check('a NaN score is not a score', () => {
  const r = replayQueue(log(proposed('a1', { scores: { ...SCORES, taste: Number.NaN } })))
  return !isScored(r.proposals[0])
})

check('an unknown event type is counted, not silently dropped', () => {
  const r = replayQueue(
    log(proposed('a1'), JSON.stringify({ type: 'promoted', at: 'T1', id: 'a1' })),
  )
  // A log written by a newer agent renders as incomplete rather than as confidently wrong.
  return r.unknownEvents === 1 && r.proposals[0].status === 'pending'
})

check('a line that is not JSON is counted as malformed', () => {
  const r = replayQueue(log(proposed('a1'), 'not json at all'))
  return r.malformedLines === 1 && r.events === 1
})

check('an event for an unknown proposal is a truncated log, not an error', () => {
  // Perfectly ordinary: somebody sent the tail of their queue.
  const r = replayQueue(log(JSON.stringify({ type: 'posted', at: 'T', id: 'gone', postedId: '1' })))
  return r.proposals.length === 0 && r.unknownEvents === 0
})

console.log('\n── what each decision taught ────────────────────────────')

check('an approval teaches one positive label', () => {
  const r = replayQueue(
    log(proposed('a1'), JSON.stringify({ type: 'decided', at: 'T', id: 'a1', status: 'approved', by: 'o' })),
  )
  const labels = labelsFor(r.proposals[0])
  return labels.length === 1 && labels[0].taste === 1 && labels[0].salience === 1
})

check('a rejection teaches one negative label and asserts no salience', () => {
  const r = replayQueue(
    log(proposed('a1'), JSON.stringify({ type: 'decided', at: 'T', id: 'a1', status: 'rejected', by: 'o' })),
  )
  const labels = labelsFor(r.proposals[0])
  // A rejection says the operator would not post it. It says nothing about whether the
  // subject mattered, so claiming salience 0 would be inventing a second opinion.
  return labels.length === 1 && labels[0].taste === 0 && labels[0].salience === undefined
})

check('an edit teaches two labels with opposite signs', () => {
  const r = replayQueue(
    log(
      proposed('a1'),
      JSON.stringify({
        type: 'decided', at: 'T', id: 'a1', status: 'approved', by: 'o', editedText: 'better',
      }),
    ),
  )
  const labels = labelsFor(r.proposals[0])
  assert(labels.length === 2, `expected 2 labels, got ${labels.length}`)
  return (
    labels[0].taste === 0 &&
    labels[0].text === 'draft a1' &&
    labels[1].taste === 1 &&
    labels[1].text === 'better'
  )
})

check('an edit that changed nothing is one label, not two', () => {
  const r = replayQueue(
    log(
      proposed('a1'),
      JSON.stringify({
        type: 'decided', at: 'T', id: 'a1', status: 'approved', by: 'o', editedText: 'draft a1',
      }),
    ),
  )
  return labelsFor(r.proposals[0]).length === 1
})

check('a pending draft teaches nothing', () => {
  const r = replayQueue(log(proposed('a1')))
  // An undecided draft is not a rejected one, and counting it as one would hand the net a
  // negative label for every draft the operator simply has not got to yet.
  return labelsFor(r.proposals[0]).length === 0
})

check('an expired draft teaches nothing', () => {
  const r = replayQueue(
    log(proposed('a1'), JSON.stringify({ type: 'expired', at: 'T', id: 'a1' })),
  )
  return labelsFor(r.proposals[0]).length === 0
})

check('a failed post still teaches, because the operator did decide', () => {
  const r = replayQueue(
    log(
      proposed('a1'),
      JSON.stringify({ type: 'decided', at: 'T', id: 'a1', status: 'approved', by: 'o' }),
      JSON.stringify({ type: 'failed', at: 'T2', id: 'a1', error: 'rate limited' }),
    ),
  )
  // The approval happened. X refusing the post afterwards is not the operator changing
  // their mind, and discarding the label would lose a real preference to an outage.
  const p = r.proposals[0]
  return p.status === 'failed' && p.error === 'rate limited'
})

check('label totals count both halves of an edit', () => {
  const r = replayQueue(
    log(
      proposed('a1'),
      JSON.stringify({ type: 'decided', at: 'T', id: 'a1', status: 'approved', by: 'o', editedText: 'x' }),
      proposed('a2'),
      JSON.stringify({ type: 'decided', at: 'T', id: 'a2', status: 'rejected', by: 'o' }),
    ),
  )
  const totals = labelTotals(r.proposals)
  return totals.positive === 1 && totals.negative === 2 && totals.fromEdits === 1
})

console.log('\n── the page ─────────────────────────────────────────────')

const terminal = readFileSync(join(HERE, '..', 'components', 'agent', 'AgentTerminal.tsx'), 'utf8')

/**
 * The file with its comments removed.
 *
 * Scanned instead of the raw text because the file *documents* the rule it obeys — it
 * says in a comment that there is no `/api/omni-agent` route — and a source scan that
 * cannot tell a prohibition from its own description fails on the honest version and
 * passes on a silent one.
 */
const code = terminal.replace(/\/\*[\s\S]*?\*\//g, '').replace(/^\s*\/\/.*$/gm, '')

check('the page never fetches', () => {
  // The same rule as /omni, applied to a more sensitive file: the queue is the operator's
  // own decision history, and a page that had to upload it to read it would be asking for
  // trust in order to avoid asking for trust.
  assert(!/\bfetch\s*\(/.test(code), 'a fetch() reached the page')
  assert(!/['"`]\/api\//.test(code), 'the page names an /api route')
  return true
})

check('the page states that it is not Scematica Omni', () => {
  return /THIS IS NOT SCEMATICA OMNI/.test(terminal) && /not a verifier/.test(terminal)
})

check('the page says an unscored draft was not rated zero', () => {
  return /it was not rated zero, it was not rated/.test(terminal)
})

check('the page shows counts, and says why not a rate', () => {
  return /Counts, not a rate/.test(terminal) && !/toFixed\(1\)\s*\+\s*'%'/.test(terminal)
})

check('there is no route serving this page data', () => {
  // Asserted rather than assumed: the easiest way to lose the no-server property is for
  // somebody to add a convenience endpoint that reads the log server-side.
  return !existsSync(join(HERE, '..', 'app', 'api', 'omni-agent'))
})

console.log(`\n${checks - failures}/${checks} checks passed`)
if (failures > 0) process.exit(1)
