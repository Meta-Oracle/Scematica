#!/usr/bin/env node
// Pin the mesh's pure view logic: the layer table, the colour rules, tri-state edges and
// layout stability.
//
// The Rust crate owns the mathematics and tests it there (39 cases). What can only go
// wrong on this side is the *rendering* of honesty: a tone that makes stale data look
// current, an unknown veto drawn as a cleared one, or a layer table that silently drops a
// node kind into column 0. Those are the cases here.
//
//   node scripts/check-mesh.mjs        (Node 22+; types are stripped natively)

import {
  LAYER,
  NODE_H,
  NODE_W,
  TONE_HEX,
  ageLabel,
  edgeBlocking,
  edgePath,
  humanise,
  layout,
  statusOf,
  toneFor,
  visibilityLabel,
} from '../lib/mesh/view.ts'

let failed = 0
const check = (name, ok) => {
  if (!ok) failed++
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}`)
}

const node = (id, kind, provenance, verdict = 'pass', activity = null) => ({
  id,
  kind,
  label: id,
  blurb: '',
  provenance,
  verdict,
  activity,
  detail: [],
  reason: null,
})

const LIVE = { kind: 'live', age_secs: 3 }
const STALE = { kind: 'stale', age_secs: 900_000, budget_secs: 30 }
const ABSENT = { kind: 'absent' }

console.log('── layer table ───────────────────────────────────────────')

// Mirrors NodeKind::layer() in Rust. A kind missing here would stack in column 0 and the
// picture would stop matching the pipeline.
const KINDS = ['listener', 'filter', 'scorer', 'breaker', 'learner', 'reasoner', 'gate', 'executor', 'peer']
check('every node kind has a layer', KINDS.every(k => typeof LAYER[k] === 'number'))
check('the table has no extra kinds', Object.keys(LAYER).length === KINDS.length)
check('flow runs left to right', LAYER.listener < LAYER.filter && LAYER.filter < LAYER.breaker)
check('execution is downstream of cognition', LAYER.learner < LAYER.executor)
check('filters and scorers share a column', LAYER.filter === LAYER.scorer)

console.log('\n── colour rules (the honesty layer) ──────────────────────')

check('a live node reads live', toneFor(node('a', 'learner', LIVE)) === 'live')
check('an absent node reads absent', toneFor(node('a', 'learner', ABSENT)) === 'absent')
// The rule that matters most: a STALE node claiming PASS has not passed anything
// recently, and painting it the same green as a live pass is the whole failure mode.
check('a stale PASS is not painted as live', toneFor(node('a', 'learner', STALE, 'pass')) === 'stale')
check('a live VETO is the alarm tone', toneFor(node('a', 'learner', LIVE, 'veto')) === 'veto')
// …and a stale veto is history, not an alarm. Painting it red sends an operator hunting a
// gate that may have opened months ago.
check('a stale VETO is stale, not alarm', toneFor(node('a', 'learner', STALE, 'veto')) === 'stale')
check('an absent node can never be the alarm tone', toneFor(node('a', 'learner', ABSENT, 'veto')) === 'absent')
check('every tone has a distinct colour', new Set(Object.values(TONE_HEX)).size === Object.keys(TONE_HEX).length)

console.log('\n── edges are tri-state ───────────────────────────────────')

const edge = (kind, active) => ({ from: 'a', to: 'b', kind, active, label: null })
check('an active veto blocks', edgeBlocking(edge('veto', true)) === true)
check('a cleared veto does not block', edgeBlocking(edge('veto', false)) === false)
// An unexamined veto must stay distinguishable from a cleared one all the way to the
// renderer, or the UI reports a gate as open that nobody looked at.
check('an unknown veto stays null, not false', edgeBlocking(edge('veto', null)) === null)
check('a signal edge never blocks', edgeBlocking(edge('signal', true)) === false)

console.log('\n── provenance labels ─────────────────────────────────────')

check('status passes the discriminant through', statusOf(LIVE) === 'live' && statusOf(ABSENT) === 'absent')
check('an absent node has no age', ageLabel(ABSENT) === null)
check('a live node has an age', ageLabel(LIVE) === '3s')
check('ages coarsen upward', humanise(45) === '45s' && humanise(600) === '10m' && humanise(7200) === '2h' && humanise(200000) === '2d')

console.log('\n── layout ────────────────────────────────────────────────')

const mesh = {
  nodes: [
    node('listener.pools', 'listener', LIVE),
    node('filter.a', 'filter', LIVE),
    node('filter.b', 'filter', LIVE),
    node('learner.dqstar', 'learner', LIVE, 'veto'),
    node('exec.executor', 'executor', LIVE),
  ],
  edges: [
    { from: 'listener.pools', to: 'filter.a', kind: 'signal', active: true, label: null },
    { from: 'learner.dqstar', to: 'exec.executor', kind: 'veto', active: true, label: null },
    // Dangling on purpose: `Mesh::validate` in Rust is what should catch this, and the
    // renderer's job is to drop it rather than draw a line into empty space.
    { from: 'learner.dqstar', to: 'ghost', kind: 'signal', active: true, label: null },
  ],
  generated_at: 't',
  summary: {
    nodes_total: 5, nodes_live: 5, nodes_stale: 0, nodes_absent: 0, nodes_simulated: 0,
    visibility: 1, blocking: 1, blocking_stale: 0, diagnosis: 'x',
  },
  cognition: null,
}

const L = layout(mesh)
check('every node is placed', L.placed.length === 5)
check('the two filters share a column', L.placed[1].x === L.placed[2].x)
check('and are on different rows', L.placed[1].y !== L.placed[2].y)
check('the listener is left of the executor', L.placed[0].x < L.placed[4].x)
// A dangling edge is dropped rather than drawn to (0,0), where it would look like a real
// connection to whatever node happens to sit there.
check('a dangling edge is dropped, not drawn', L.edges.length === 2)
check('the canvas encloses every node', L.placed.every(p => p.x + NODE_W <= L.width && p.y + NODE_H <= L.height))
check('columns are labelled', L.columns.length === 4 && L.columns[0].title === 'Ingest')

// Layout must be stable across polls: a node that jumps rows when an unrelated one
// appears makes the graph unreadable regardless of how it looks in a screenshot.
const again = layout(mesh)
check('layout is deterministic', JSON.stringify(again.placed.map(p => [p.x, p.y])) === JSON.stringify(L.placed.map(p => [p.x, p.y])))

check('an edge path is a bezier from the source', edgePath(L.edges[0]).startsWith(`M ${L.edges[0].x1} ${L.edges[0].y1} C`))

console.log('\n── headline ──────────────────────────────────────────────')

check('the visibility line leads with the percentage', visibilityLabel(mesh).startsWith('100% visible'))
check(
  'and names all three states',
  ['live', 'stale', 'unseen'].every(w => visibilityLabel(mesh).includes(w)),
)

console.log(`\n${failed === 0 ? 'ALL PASS' : `${failed} FAILED`}`)
process.exit(failed === 0 ? 0 : 1)
