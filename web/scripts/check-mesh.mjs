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
  trace,
} from '../lib/mesh/view.ts'
import {
  TAU_PSI,
  TAU_PSI_FULL,
  dominantConstraint,
  effective,
  recompute,
  sensitivities,
  verdictFor,
} from '../lib/mesh/gate.ts'
import { normalizeBase } from '../lib/net.ts'

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
const KINDS = ['listener', 'filter', 'scorer', 'breaker', 'learner', 'reasoner', 'gate', 'executor', 'peer', 'agent']
check('every node kind has a layer', KINDS.every(k => typeof LAYER[k] === 'number'))
check('the table has no extra kinds', Object.keys(LAYER).length === KINDS.length)
check('flow runs left to right', LAYER.listener < LAYER.filter && LAYER.filter < LAYER.breaker)
check('execution is downstream of cognition', LAYER.learner < LAYER.executor)
check('filters and scorers share a column', LAYER.filter === LAYER.scorer)
// An `agent` node is a decision runtime watching from outside — Scematica Omni. It sits in
// the cognition column with the learners, and it has no edges at all: omni perceives, ranks
// and records, and nothing in it writes to the environment it observed. Placing it
// downstream of execution would imply a wire into the trading path that does not exist.
check('an observing agent sits with cognition', LAYER.agent === LAYER.learner)
check('an observing agent is upstream of execution', LAYER.agent < LAYER.executor)

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

console.log('\n── trace ─────────────────────────────────────────────────')

const t = trace(mesh, 'exec.executor')
check('the traced set includes the selection', t.nodes.has('exec.executor'))
check('and everything upstream of it', t.nodes.has('learner.dqstar'))
check('but not an unconnected node', !t.nodes.has('filter.b'))
check('the connecting edge is traced', t.edges.has('learner.dqstar->exec.executor:veto'))
// The graph has feedback edges (promotion runs backwards into the primary learner), so an
// unguarded walk would not terminate. This asserts it does.
const cyclic = {
  ...mesh,
  edges: [
    { from: 'a', to: 'b', kind: 'signal', active: true, label: null },
    { from: 'b', to: 'a', kind: 'promotion', active: true, label: null },
  ],
  nodes: [node('a', 'learner', LIVE), node('b', 'learner', LIVE)],
}
check('a cycle terminates', trace(cyclic, 'a').nodes.size === 2)

console.log('\n── gate parity with Rust ─────────────────────────────────')

const term = (symbol, section, value, measured) => ({ symbol, section, name: symbol, value, measured, note: '' })

// Fixture taken from an actual `cargo run --example dump` against the real state files:
// R_model 0.207, R_dd 0.198, R_liq 0.000, R_vol 1.000 measured; R_exec and R_conc absent.
// Rust reported C 1.000, K 1.000, R 0.351, Ψ 0.649.
const REAL = {
  confidence: 1,
  confidence_terms: [
    term('U_A', '§16', 0, false),
    term('U_E', '§16', 0, false),
    term('N_t', '§14', 0, false),
    term('D_t', '§40', 0, false),
  ],
  uncertainty: { aleatoric: term('U_A', '§16', 0, false), epistemic: term('U_E', '§16', 0, false), total: 0 },
  risk: {
    components: [
      term('R_model', '§20', 0.20692229554528332, true),
      term('R_exec', '§20', 0, false),
      term('R_dd', '§20', 0.198, true),
      term('R_liq', '§20', 0, true),
      term('R_vol', '§20', 1, true),
      term('R_conc', '§20', 0, false),
    ],
    value: 0.351,
  },
  coherence: { value: 1, subsystems: 0, disagreement: 0, approximation: true, note: '' },
  psi: 0.649,
  verdict: 'unevaluated',
  omega: null,
  omega_terms: [term('H_t', '§3', 0, false), term('M_t', '§11', 0, false)],
  measured_fraction: 4 / 12,
  reading: '',
}

const base = recompute(REAL, {})
check('confidence matches Rust', Math.abs(base.confidence - 1) < 1e-9)
// The rule that matters: the mean is over MEASURED components only. Averaging in the two
// unmeasured zeros would give 0.234 and read as materially safer than the truth.
check('risk averages over measured components only', Math.abs(base.risk - 0.35123057388632086) < 1e-6)
check('and NOT over all six', Math.abs(base.risk - 1.4049222955452833 / 6) > 0.1)
check('psi matches Rust to 3dp', base.psi.toFixed(3) === '0.649')
check('a gate with no live subsystem is unevaluated', base.verdict === 'unevaluated')
check('the observed state is not dirty', base.dirty === false)

console.log('\n── counterfactual solver ─────────────────────────────────')

const lifted = recompute(REAL, { R_vol: 0 })
check('dropping volatility risk raises psi', lifted.psi > base.psi)
check('and marks the result dirty', lifted.dirty === true)
check('the observed payload is unmutated', REAL.risk.components[4].value === 1)

// Overriding an UNMEASURED term makes it count as measured, which changes the denominator.
// That is a real property of the design and the page says so out loud.
const withExec = recompute(REAL, { R_exec: 0 })
check('instrumenting a healthy term can still move risk', withExec.risk !== base.risk)
check('effective() reports an override as measured', effective(term('R_exec', '§20', 0, false), { R_exec: 0.5 }).measured === true)
check('and passes the overridden value through', effective(term('R_exec', '§20', 0, false), { R_exec: 0.5 }).value === 0.5)

const s = sensitivities(REAL, {})
check('sensitivities are ranked by magnitude', Math.abs(s[0].gradient) >= Math.abs(s[s.length - 1].gradient))
// A MEASURED risk term is a pure cost: raising it can only lower Ψ.
check(
  'raising a measured risk term lowers psi',
  s.filter(x => x.symbol.startsWith('R_') && x.measured).every(x => x.gradient <= 0),
)
check('raising an uncertainty term lowers psi', s.filter(x => x.symbol === 'U_E')[0].gradient <= 0)
// …but an UNMEASURED one behaves the other way, and that is the design rather than a bug:
// instrumenting a healthy subsystem enlarges the denominator of the risk mean, so measured
// risk falls and Ψ rises. Pinned here because it is surprising enough that someone will
// eventually "fix" it into averaging over all six, which would report 0.234 for a field
// whose measured components average 0.351.
const rExec = s.find(x => x.symbol === 'R_exec')
check('an unmeasured healthy term raises psi when instrumented', !rExec.measured && rExec.gradient > 0)

check('thresholds match Rust', TAU_PSI === 0.45 && TAU_PSI_FULL === 0.75)
check('verdict boundaries are exclusive at tau', verdictFor(0.45, true, 1) === 'damp' && verdictFor(0.4499, true, 1) === 'abstain')
check('full conviction needs tau_full', verdictFor(0.75, true, 1) === 'act' && verdictFor(0.7499, true, 1) === 'damp')
// Nothing measured is ignorance, never a considered refusal.
check('nothing measured is unevaluated, not abstain', verdictFor(0.0, false, 0) === 'unevaluated')

// Unevaluated is not the same as unmeasured: K has no live subsystem, but four risk terms
// are well measured, and the line must say so rather than claiming nothing is known.
const dom = dominantConstraint(REAL, {})
check('the dominant constraint names a measured term', /R_(model|dd|vol|liq)/.test(dom))
check('and never names an unmeasured one', !/U_A|N_t/.test(dom))
check('it distinguishes no-verdict from no-evidence', /no verdict/.test(dom))
check(
  'with nothing measured at all it says so',
  /absence of evidence/.test(dominantConstraint({ ...REAL, risk: { components: [term('R_x', '§20', 0, false)], value: 0 } }, {})),
)

console.log('\n── transport: where the request actually goes ─────────────')

// The bug this section exists for: `/mesh` on a hosted deploy rendered "No instance
// paired" against a perfectly healthy bot, because the paired base was `https://host/api`
// and every caller appends `/api/...` — so the request went to `/api/api/mesh` and 404'd.
// It was undetectable because the Rust router serves BOTH `/health` and `/api/health`, so
// the old pairing probe (which checked `<base>/health`) reported success.
//
// Two independent guarantees are pinned here: the base is repaired, and the probe path is
// one that a wrong root cannot satisfy.
check('a trailing /api is stripped from the base', normalizeBase('https://host/api') === 'https://host')
check('trailing slashes go too', normalizeBase('https://host/api/') === 'https://host')
check('and both together', normalizeBase('  https://host/api///  ') === 'https://host')
check('a correct root is left alone', normalizeBase('https://host') === 'https://host')
check('a port survives', normalizeBase('http://192.168.1.50:3001') === 'http://192.168.1.50:3001')
// Only a *trailing* /api is a mistake. A reverse proxy legitimately mounted at
// `https://host/api/bot` must keep its path, or the fix breaks a working deploy.
check('an interior /api is not touched', normalizeBase('https://host/api/bot') === 'https://host/api/bot')
// Idempotent, because it now runs on write, on read, and inside the probe.
check('normalising twice changes nothing', normalizeBase(normalizeBase('https://host/api/')) === 'https://host')

// The alias that made the original bug silent. If a future refactor points the probe back
// at `/health`, a mis-rooted base passes again — so assert the shape of the path itself.
const probeSrc = await import('node:fs').then(fs =>
  fs.readFileSync(new URL('../lib/net.ts', import.meta.url), 'utf8'),
)
check(
  'the pairing probe checks /api/health, not the aliased /health',
  /fetch\(base \+ '\/api\/health'/.test(probeSrc) && !/fetch\(base \+ '\/health'/.test(probeSrc),
)

console.log(`\n${failed === 0 ? 'ALL PASS' : `${failed} FAILED`}`)
process.exit(failed === 0 ? 0 : 1)
