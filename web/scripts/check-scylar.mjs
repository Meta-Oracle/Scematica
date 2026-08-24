#!/usr/bin/env node
// Pin the Scylar terminal's pure logic: the expression state machine, the markdown
// parser, slash-command parsing, and transcript (de)serialisation.
//
// These four modules were written as pure functions specifically so they could be
// checked without a browser, a provider key, or a running bot. Everything genuinely
// stateful — the rAF loop, the SSE reader, localStorage — lives in components and is not
// covered here; what is covered is the part where a wrong constant or an inverted
// comparison produces a silently wrong avatar or a mangled code block.
//
//   node scripts/check-scylar.mjs        (Node 22+; types are stripped natively)

import {
  EXPRESSIONS,
  FLAP_OPEN_RATIO,
  FLAP_PERIOD_MS,
  FLAP_CROSSFADE_MS,
  EXPRESSION_CROSSFADE_MS,
  REACTION_HOLD_MS,
  presenceFor,
  readsPositive,
  spriteFor,
  spriteSrc,
} from '../lib/scylar/expressions.ts'
import { parseInline, parseMarkdown } from '../lib/scylar/markdown.ts'
import {
  VOICE_PITCH,
  VOICE_RATE,
  estimateWordMs,
  pickVoice,
  pickedFemaleVoice,
  speakableText,
  splitForSpeech,
  wordAt,
} from '../lib/scylar/speech.ts'
import { parseCommand } from '../lib/scylar/commands.ts'
import { deserialise, serialise } from '../lib/scylar/session.ts'
import { contextSystemMessage } from '../lib/scylar/context.ts'
import { SEAL_TOOL, TOOLS, availableTools, runTool, toolDefinitions } from '../lib/scylar/tools.ts'
import { OMNI_INSTRUCTION } from '../lib/scylar/omni.ts'
import {
  cautionInstruction,
  holdExplanation,
  holdInstruction,
  readGate,
} from '../lib/scylar/gate.ts'
import { CODEX, codexEntry, codexIds, codexMap, lookup, searchCodex } from '../lib/scylar/codex.ts'
import { DEFAULT_BUDGET, PSYCHE, composePsyche } from '../lib/scylar/psyche.ts'
import {
  CENTER,
  CHANNEL_START,
  RADIUS,
  READOUT,
  arcPath,
  channelRole,
  coverageCells,
  gaugeArc,
  motionFor,
  polar,
  channelPositions,
  sigilView,
  ticks,
  tracePoints,
} from '../lib/scylar/sigil.ts'
import { existsSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

/** Repository root, from this file's location — `web/scripts/` is two levels down. */
const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..')

let failed = 0
const check = (name, ok) => {
  if (!ok) failed++
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}`)
}

console.log('── expressions ───────────────────────────────────────────')

check('idle phase shows idle', spriteFor({ kind: 'idle' }) === 'idle')
check('thinking holds a closed mouth', spriteFor({ kind: 'thinking' }) === 'idle')

// The flap is duty-cycled, not a 50/50 alternation — the mouth is open for the larger
// share of each cycle. Boundaries are what a wrong ratio breaks first.
const flapAt = (ms) => spriteFor({ kind: 'streaming', elapsedMs: ms })
const openUntil = FLAP_PERIOD_MS * FLAP_OPEN_RATIO
check('flap opens at cycle start', flapAt(0) === 'talking')
check('flap still open just before the boundary', flapAt(openUntil - 1) === 'talking')
check('flap closes at the boundary', flapAt(openUntil + 1) === 'idle')
check('flap closed at end of cycle', flapAt(FLAP_PERIOD_MS - 1) === 'idle')
check('flap reopens on the next cycle', flapAt(FLAP_PERIOD_MS) === 'talking')
check('flap is open for more than half the cycle', FLAP_OPEN_RATIO > 0.5)

// The crossfade has to finish inside the cycle. Longer than the period and both sprites
// stay permanently half-lit, which reads as a blur rather than a mouth.
check('flap crossfade fits inside the flap period', FLAP_CROSSFADE_MS < FLAP_PERIOD_MS)
check('mood crossfade is slower than the flap', EXPRESSION_CROSSFADE_MS > FLAP_CROSSFADE_MS)

check(
  'positive settle reacts',
  spriteFor({ kind: 'settled', positive: true, sinceMs: 0 }) === 'joyous',
)
check(
  'reaction decays after the hold',
  spriteFor({ kind: 'settled', positive: true, sinceMs: REACTION_HOLD_MS }) === 'idle',
)
check(
  'neutral settle does not react',
  spriteFor({ kind: 'settled', positive: false, sinceMs: 0 }) === 'idle',
)

// Presence is the slow half of the animation. These assert direction, not values: the
// numbers are taste, the ordering is the design.
const pIdle = presenceFor({ kind: 'idle' })
const pThink = presenceFor({ kind: 'thinking' })
const pStream = presenceFor({ kind: 'streaming', elapsedMs: 0 })
const pReact = presenceFor({ kind: 'settled', positive: true, sinceMs: 0 })

check('thinking draws back from idle', pThink.scale < pIdle.scale && pThink.lift > pIdle.lift)
check('speaking leans in', pStream.lift < pIdle.lift && pStream.scale > pIdle.scale)
check('reacting is the strongest pose', pReact.scale > pStream.scale && pReact.lift < pStream.lift)
check('glow rises idle → thinking → speaking → reacting',
  pIdle.glow < pThink.glow && pThink.glow < pStream.glow && pStream.glow < pReact.glow)
check('reaction presence decays with the sprite',
  presenceFor({ kind: 'settled', positive: true, sinceMs: REACTION_HOLD_MS }).scale === pIdle.scale)

check('negatives outrank positives', readsPositive('Great, but it failed.') === false)
check('plain positive reads positive', readsPositive('Nice work, that lands.') === true)
check('neutral reads neutral', readsPositive('The pool is 40 SOL deep.') === false)
check('apology is not positive', readsPositive("Sorry — I can't reach the bot.") === false)

check('every expression has a 512 and 1024 asset path',
  EXPRESSIONS.every((e) => spriteSrc(e, 512).endsWith(`${e}-512.webp`) &&
                            spriteSrc(e, 1024).endsWith(`${e}-1024.webp`)))

// Voicing is one open-close per word, sized to that word — not a free-running cycle.
const voice = (since, wordMs) => spriteFor({ kind: 'voicing', sinceWordMs: since, wordMs })
check('mouth opens on a word', voice(0, 300) === 'talking')
check('mouth closes before the word ends', voice(299, 300) === 'idle')
check('a long word holds the mouth open longer than a short one',
  voice(200, 500) === 'talking' && voice(200, 220) === 'idle')
check('voicing carries the speaking pose',
  presenceFor({ kind: 'voicing', sinceWordMs: 0, wordMs: 300 }).lift === pStream.lift)

console.log('\n── speech ────────────────────────────────────────────────')

// Reading punctuation aloud is what makes a synthetic voice sound broken.
const spoken = speakableText('Use **bold** and `code` and [docs](https://x.dev) here.')
check('emphasis markers are not spoken', !spoken.includes('**'))
check('backticks are not spoken', !spoken.includes('`'))
check('a link keeps its label', spoken.includes('docs'))
check('a link URL is not read aloud', !spoken.includes('https'))

// A skipped code block is announced, not silently dropped — otherwise the spoken answer
// has a hole the listener cannot account for.
const withCode = speakableText('Here:\n\n```rust\nfn main() {}\n```\n\nDone.')
check('a code block becomes a spoken stand-in', /rust code block/.test(withCode))
check('code contents are not read out', !withCode.includes('fn main'))
check('an unterminated code block is still handled',
  /code block/.test(speakableText('```ts\nconst x = 1')))
check('headings lose their hashes', !speakableText('### Commands').includes('#'))

// Chrome goes silent after ~15s of one utterance and reports nothing, so chunking is a
// correctness requirement rather than a nicety.
const long = splitForSpeech('One. Two. ' + 'word '.repeat(200) + 'End.')
check('long text is split', long.length > 1)
check('every chunk stays under the utterance cap', long.every((c) => c.length <= 200))
check('no chunk is empty', long.every((c) => c.trim().length > 0))
check('short text stays one chunk', splitForSpeech('Just this.').length === 1)
check('empty text produces no chunks', splitForSpeech('   ').length === 0)
// A single sentence longer than the cap has no boundary to split on and must still break.
check('an unpunctuated run is still broken up',
  splitForSpeech('x'.repeat(900)).every((c) => c.length <= 200))

check('word duration grows with length',
  estimateWordMs('a') < estimateWordMs('extraordinarily'))
check('word duration is clamped at both ends', (() => {
  const all = ['a', 'the', 'antidisestablishmentarianism', 'x'.repeat(80)]
  return all.every((w) => estimateWordMs(w) >= 130 && estimateWordMs(w) <= 620)
})())
check('a faster rate shortens a word', estimateWordMs('trading', 2) < estimateWordMs('trading', 1))
check('wordAt reads the word at an offset', wordAt('the pool score', 4) === 'pool')
check('wordAt handles the final word', wordAt('the pool score', 9) === 'score')
check('wordAt is safe past the end', wordAt('short', 99) === '')

check('no voices yields no choice', pickVoice([]) === null)
check('an English voice is preferred', (() => {
  const v = pickVoice([
    { name: 'Sprecher', lang: 'de-DE', default: true },
    { name: 'Microsoft Aria Online (Natural) - English (United States)', lang: 'en-US', default: false },
  ])
  return v.name.includes('Aria')
})())
check('a non-English list still yields a voice',
  pickVoice([{ name: 'Sprecher', lang: 'de-DE', default: true }]).name === 'Sprecher')

// The exact regression. A stock Windows + Edge list: quality-first ranking picked the
// male natural voice over Zira, because "natural" matched before any name did.
const windowsEdge = [
  { name: 'Microsoft David Desktop - English (United States)', lang: 'en-US', default: true },
  { name: 'Microsoft Zira Desktop - English (United States)', lang: 'en-US', default: false },
  { name: 'Microsoft Andrew Online (Natural) - English (United States)', lang: 'en-US', default: false },
]
check('a female voice beats a higher-quality male one',
  pickVoice(windowsEdge).name.includes('Zira'))
check('the default voice does not win by being the default',
  !pickVoice(windowsEdge).name.includes('David'))

// Quality is still the tiebreak — among female voices only.
check('quality decides between two female voices',
  pickVoice([
    { name: 'Microsoft Zira Desktop - English (United States)', lang: 'en-US', default: true },
    { name: 'Microsoft Emma Online (Natural) - English (United States)', lang: 'en-US', default: false },
  ]).name.includes('Emma'))

check('macOS names are recognised',
  pickVoice([
    { name: 'Daniel', lang: 'en-GB', default: true },
    { name: 'Samantha', lang: 'en-US', default: false },
  ]).name === 'Samantha')
check('an explicit "female" label is honoured',
  pickVoice([
    { name: 'English (United States) male', lang: 'en-US', default: true },
    { name: 'English (United States) female', lang: 'en-US', default: false },
  ]).name.endsWith('female'))
// "male" is a substring of "female"; a naive check gets this exactly backwards.
check('"female" is not read as "male"',
  pickedFemaleVoice({ name: 'English (United States) female', lang: 'en-US' }) === true)

// Whole tokens, not substrings: "Ava" inside "Avalon", "Ana" inside "Anatoly".
check('a name is matched as a whole word',
  pickedFemaleVoice({ name: 'Microsoft Avalon Desktop', lang: 'en-US' }) === false)

// Degrading is mandatory — an unrecognised list must still speak, and must say it could
// not confirm the choice rather than silently sounding wrong.
const unknown = pickVoice([{ name: 'Voice 1', lang: 'en-US', default: true }])
check('an unrecognised list still yields a voice', unknown.name === 'Voice 1')
check('an unconfirmed pick is reported as such', pickedFemaleVoice(unknown) === false)

check('delivery is pitched below neutral', VOICE_PITCH < 1)
check('delivery is measured rather than clipped', VOICE_RATE < 1 && VOICE_RATE > 0.9)

console.log('\n── markdown ──────────────────────────────────────────────')

const fenced = parseMarkdown('before\n\n```rust\nfn main() {}\n```\n\nafter')
check('fenced block is extracted', fenced.length === 3 && fenced[1].kind === 'code')
check('fence language survives', fenced[1].lang === 'rust')
check('fence body excludes the fences', fenced[1].text === 'fn main() {}')
check('closed fence is not open', fenced[1].open === false)

// Streaming: the fence arrives long before its closer. Rendering the partial as literal
// text and then snapping it into a block is the jarring version of this.
const partial = parseMarkdown('```ts\nconst x = 1')
check('unterminated fence is a block in progress',
  partial.length === 1 && partial[0].kind === 'code' && partial[0].open === true)

// Precedence: a code span swallows emphasis. Half this conversation is about code, so
// getting this backwards would mangle most answers containing a pointer type.
const inline = parseInline('use `**ptr**` here')
check('code span suppresses emphasis inside it',
  inline.some((n) => n.kind === 'code' && n.text === '**ptr**') &&
  !inline.some((n) => n.kind === 'strong'))
check('emphasis outside code still parses',
  parseInline('**bold** text').some((n) => n.kind === 'strong' && n.text === 'bold'))

const safe = parseInline('see [docs](https://scematica.org)')
check('https link is a link', safe.some((n) => n.kind === 'link' && n.href === 'https://scematica.org'))

const unsafe = parseInline('click [here](javascript:alert(1))')
check('javascript: URL is never a link', !unsafe.some((n) => n.kind === 'link'))
check('unsafe URL is still shown as text',
  unsafe.some((n) => n.kind === 'text' && n.text.includes('javascript:alert(1)')))

const list = parseMarkdown('- one\n- two\n\n1. first\n2. second')
check('bullets group into one list', list[0].kind === 'list' && list[0].items.length === 2)
check('ordered list is separate from the bullets',
  list[1].kind === 'list' && list[1].ordered === true && list[1].items.length === 2)
check('bullet list is not marked ordered', list[0].ordered === false)

const heading = parseMarkdown('### Commands\ntext')
check('heading level is captured', heading[0].kind === 'heading' && heading[0].level === 3)

console.log('\n── commands ──────────────────────────────────────────────')

check('/help parses', parseCommand('/help').kind === 'help')
check('/clear parses', parseCommand('/clear').kind === 'clear')
check('/retry parses', parseCommand('/retry').kind === 'retry')
check('/context on enables', (() => {
  const c = parseCommand('/context on')
  return c.kind === 'context' && c.enabled === true
})())
check('/context off disables', (() => {
  const c = parseCommand('/context off')
  return c.kind === 'context' && c.enabled === false
})())
check('bare /context toggles', (() => {
  const c = parseCommand('/context')
  return c.kind === 'context' && c.enabled === 'toggle'
})())
check('/status becomes a state-backed question', (() => {
  const c = parseCommand('/status')
  return c.kind === 'ask' && c.prompt.length > 0
})())
// A leading slash is not enough. Sending this as a failed command would be worse than
// just answering it.
check('a sentence starting with a path is not a command',
  parseCommand('/tmp is fine on Linux, right?').kind === 'none')
check('ordinary text is not a command', parseCommand('what is fractional Kelly?').kind === 'none')

console.log('\n── session ───────────────────────────────────────────────')

check('garbage deserialises to empty', deserialise('not json at all').length === 0)
check('null deserialises to empty', deserialise(null).length === 0)
check('non-array deserialises to empty', deserialise('{"role":"user"}').length === 0)
check('malformed turns are dropped',
  deserialise(JSON.stringify([{ role: 'user', content: 'ok' }, { role: 'ghost' }, { content: 5 }]))
    .length === 1)
check('round-trip preserves a finished exchange', (() => {
  const turns = [
    { role: 'user', content: 'hi', done: true },
    { role: 'assistant', content: 'hello', done: true, context: 'live' },
  ]
  const back = deserialise(serialise(turns))
  return back.length === 2 && back[1].context === 'live'
})())
// A half-streamed reply is not history; restoring one would resurrect a truncated answer
// with no indication it was cut off.
check('incomplete assistant turns are not stored',
  deserialise(serialise([{ role: 'assistant', content: 'partial…' }])).length === 0)

// A stored seal proposal drives a control that writes, so it is untrusted input in a way
// the rest of the transcript is not: localStorage is writable by anything on the origin,
// and the goal inside it is what would be sent if the operator clicked.
check('a seal proposal survives a transcript round trip',
  deserialise(serialise([{
    role: 'assistant', content: 'shall I record that?', done: true,
    sealProposal: { goal: 'clear the marker backlog', ground: ['markers:x'] },
  }]))[0].sealProposal.goal === 'clear the marker backlog')
check('a proposal with an empty goal is dropped',
  deserialise(JSON.stringify([{
    role: 'assistant', content: 'x', done: true, sealProposal: { goal: '  ', ground: [] },
  }])).length === 0)
check('a proposal with a non-string goal is dropped',
  deserialise(JSON.stringify([{
    role: 'assistant', content: 'x', done: true, sealProposal: { goal: 42, ground: [] },
  }])).length === 0)
check('a proposal with non-string grounds is dropped',
  deserialise(JSON.stringify([{
    role: 'assistant', content: 'x', done: true,
    sealProposal: { goal: 'g', ground: [{ evil: true }] },
  }])).length === 0)
check('a sealed marker survives a round trip',
  deserialise(serialise([{
    role: 'assistant', content: 'done', done: true,
    sealProposal: { goal: 'g', ground: [] }, sealed: { id: 'abc123', root: 'ff00' },
  }]))[0].sealed.id === 'abc123')

console.log('\n── context ───────────────────────────────────────────────')

const simMsg = contextSystemMessage({ source: 'simulation', text: 'PnL 0.0000 SOL' })
check('simulated briefing is labelled in the header', simMsg.includes('SIMULATION'))
// Descriptive phrasing ("say so whenever you cite them") was tested against
// llama-3.3-70b and ignored outright. Naming the required word is what got complied
// with, so the requirement — not merely a mention of simulation — is what is pinned.
check('simulated briefing states a required output token',
  /REQUIRED/.test(simMsg) && /"simulated"/.test(simMsg))
const liveMsg = contextSystemMessage({ source: 'live', text: 'PnL 1.2 SOL' })
check('live briefing is labelled LIVE', liveMsg.includes('(LIVE)'))
check('live briefing does not claim simulation', !/SIMULATION ENGINE/.test(liveMsg))
check('no briefing yields no system message',
  contextSystemMessage({ source: 'unavailable', text: null }) === null)

console.log('\n── tools ─────────────────────────────────────────────────')

// The security property, asserted rather than assumed: a model chooses a name, never a
// URL. If a tool ever grows a caller-supplied path this is the check that fails.
//
// A LOCAL tool (the codex) has no path at all, which is strictly stronger — there is no
// URL to aim at. It is excluded here and asserted separately below, rather than being
// allowed to slip through a check whose whole subject is the path.
const NETWORK_TOOLS = TOOLS.filter((t) => !t.local)
check('every network tool hard-codes its own path',
  NETWORK_TOOLS.every((t) => typeof t.path === 'string' && t.path.length > 0 && !t.path.includes('..')))
check('no tool targets a control route',
  NETWORK_TOOLS.every((t) => !t.path.startsWith('controls/') && !t.path.startsWith('push')))
// A local tool must be exactly that: no path, and a handler. Half of each is how a tool
// that was meant to answer in-process quietly acquires a network call.
check('every local tool has a handler and no path',
  TOOLS.filter((t) => t.local).every((t) => t.path === undefined && typeof t.local === 'function'))
// POST is allowed only for endpoints that compute and return. "Read-only" has to be a
// property of the list, not of the verb, or the first POST tool quietly widens it. The
// allowlist is explicit rather than a pattern, so adding a writing endpoint is a decision
// somebody has to make here in words.
const COMPUTING_POSTS = ['replay', 'scylar/omni/simulate']
check('only computing endpoints may POST',
  NETWORK_TOOLS.filter((t) => t.method === 'POST').every((t) => COMPUTING_POSTS.includes(t.path)))

// ── the write path ────────────────────────────────────────────────────────────
//
// Scylar was strictly read-only. Exactly one tool now writes, and these are the properties
// that keep that from becoming two.
check('the sealing tool is not in the general tool list',
  !TOOLS.some((t) => t.name === SEAL_TOOL.name))
check('sealing is absent unless the deployment enables it',
  !availableTools({ bot: true, omni: true, seal: false, codex: false }).some((t) => t.name === SEAL_TOOL.name))
check('sealing requires omni, not just the flag',
  !availableTools({ bot: true, omni: false, seal: true, codex: false }).some((t) => t.name === SEAL_TOOL.name))
check('sealing is offered when both are on',
  availableTools({ bot: true, omni: true, seal: true, codex: false }).some((t) => t.name === SEAL_TOOL.name))

// The confirmation must not be satisfiable by the thing asking for it. An earlier version
// set `confirm: true` in the tool body, which made the route's 428 decorative.
check('the seal tool body cannot confirm itself',
  SEAL_TOOL.body({ goal: 'x' }).confirm === undefined)

// Calling it proposes; it does not write. `runTool` intercepts the name before any fetch.
const proposed = await runTool('http://unused.invalid', SEAL_TOOL.name, JSON.stringify({ goal: 'tidy up' }),
  { bot: false, omni: true, seal: true })
check('asking to seal records a proposal instead of sealing',
  proposed.ok === true && proposed.proposal?.goal === 'tidy up')
check('a proposal tells the model nothing was written',
  /nothing has been sealed/i.test(proposed.content))
check('a seal with no goal is refused rather than proposed', (await runTool(
  'http://unused.invalid', SEAL_TOOL.name, '{}', { bot: false, omni: true, seal: true },
)).proposal === undefined)

// ── tool groups ───────────────────────────────────────────────────────────────
//
// The omni loop reasons about a source tree and does not care whether the sniper is
// running. Gating it on the bot would switch off reasoning about the codebase at exactly
// the moment an operator is most likely to be asking what to do about it.
// A tool with no group is invisible to `availableTools` and therefore silently never
// offered — a failure that looks exactly like the model choosing not to call it.
const GROUPS = ['bot', 'omni', 'codex']
check('every tool declares a known group', TOOLS.every((t) => GROUPS.includes(t.group)))
check('omni tools survive the bot being down',
  availableTools({ bot: false, omni: true, seal: false, codex: false }).every((t) => t.group === 'omni') &&
  availableTools({ bot: false, omni: true, seal: false, codex: false }).length > 0)
check('bot tools are withheld when the bot is down',
  !availableTools({ bot: false, omni: true, seal: false, codex: false }).some((t) => t.group === 'bot'))
check('no tools at all when neither subsystem is reachable',
  availableTools({ bot: false, omni: false, seal: false, codex: false }).length === 0)

// Each omni tool description has to carry the rule the model gets wrong without it. This is
// the last layer of omni's design and the only one with no type system under it.
const omniTools = TOOLS.filter((t) => t.group === 'omni')
check('the simulate tool warns that an em dash is not a zero',
  /em dash/i.test(omniTools.find((t) => t.name === 'omni_simulate').description) &&
  /0\.00/.test(omniTools.find((t) => t.name === 'omni_simulate').description))
check('the simulate tool requires coverage beside the score',
  /coverage|MEASURED/.test(omniTools.find((t) => t.name === 'omni_simulate').description))
check('the simulate tool says abstention is an answer',
  /abstention is an answer/i.test(omniTools.find((t) => t.name === 'omni_simulate').description))
check('the simulate tool forbids inferring grounding',
  /never infer|never inferred/i.test(omniTools.find((t) => t.name === 'omni_simulate').description))
check('the verify tool states what a commitment does not prove',
  /does NOT prove/.test(omniTools.find((t) => t.name === 'omni_verify').description))
check('a POST tool builds its body from arguments, never passes them through',
  TOOLS.filter((t) => t.method === 'POST').every((t) => typeof t.body === 'function'))
// The model supplies numbers; anything non-numeric must vanish rather than reach the API.
check('non-numeric replay arguments are dropped', (() => {
  const replayTool = TOOLS.find((t) => t.name === 'run_counterfactual')
  const body = replayTool.body({ min_pool_score: 'sixty', max_pool_age_secs: 30 })
  return body.min_pool_score === undefined && body.max_pool_age_secs === 30
})())
check('tool names are unique', new Set(TOOLS.map((t) => t.name)).size === TOOLS.length)
check('definitions are OpenAI-shaped',
  toolDefinitions({ bot: true, omni: true, seal: true, codex: false }).every((d) => d.type === 'function' && d.function.name && d.function.parameters))

// Models routinely ask for 500 rows. The clamp is what keeps one greedy call from
// eating the whole context window.
const decisions = TOOLS.find((t) => t.name === 'get_pool_decisions')
check('an over-large limit is clamped', Number(decisions.query({ limit: 500 }).limit) <= 40)
check('a zero limit is raised to at least 1', Number(decisions.query({ limit: 0 }).limit) >= 1)
check('a nonsense limit falls back to a default',
  Number.isFinite(Number(decisions.query({ limit: 'lots' }).limit)))
check('a missing limit still yields a query', Number(decisions.query({}).limit) > 0)

// Shaping is what keeps a tool result affordable — the raw decision rows carry ~20
// fields each, most of them irrelevant to any question worth asking.
const shaped = decisions.shape({
  decisions: [{ mint: 'abc', reason: 'fibonacci_gate', pool_score: 47, utc_hour: 3, social_count: 0 }],
})
check('shaping keeps the fields that answer questions',
  shaped[0].mint === 'abc' && shaped[0].reason === 'fibonacci_gate')
check('shaping drops the fields that do not', shaped[0].utc_hour === undefined)

// An invented tool name must recover the turn, not end it.
const ALL = { bot: true, omni: true, seal: false }
const bogus = await runTool('http://127.0.0.1:1', 'get_everything', '{}', ALL)
check('an unknown tool is refused', bogus.ok === false)
check('an unknown tool names the real ones', bogus.content.includes('get_pool_decisions'))

// An unreachable bot is information, not an exception.
const unreachable = await runTool('http://127.0.0.1:1', 'get_controls', '{}', ALL)
check('an unreachable API returns a message, not a throw',
  unreachable.ok === false && /could not reach/i.test(unreachable.content))

console.log('\n── cognitive gate ────────────────────────────────────────')

// A HOLD has to explain the thing that is actually wrong. "Ψ is below threshold" is
// true and useless; "your sniper stopped writing metrics an hour ago" is actionable.
const wedged = holdExplanation({
  verdict: 'hold', psi: 0, sentience: 0.4, bottleneck: 'logic', note: '',
  inputs: { metrics_age_secs: 7200, sniper_running: true, state_files_present: 4 },
})
check('a wedged sniper is named as such', /wedged|stale/i.test(wedged))
check('the explanation gives the age in minutes', wedged.includes('120 minute'))

const stopped = holdExplanation({
  verdict: 'hold', psi: 0, sentience: 0.4, bottleneck: 'logic', note: '',
  inputs: { metrics_age_secs: 3600, sniper_running: false, state_files_present: 4 },
})
check('a stopped sniper is described as a finished session', /finished session/i.test(stopped))
// Pluralisation: 61s is the narrow window where the singular branch is reachable.
check('one minute is not written as "1 minutes"',
  holdExplanation({
    verdict: 'hold', psi: 0, sentience: 0, bottleneck: 'logic', note: '',
    inputs: { metrics_age_secs: 61, sniper_running: false, state_files_present: 4 },
  }).includes('1 minute old'))

check('missing state files get their own explanation',
  /not readable|nothing to answer/i.test(holdExplanation({
    verdict: 'hold', psi: 0, sentience: 0, bottleneck: 'perception', note: '',
    inputs: { state_files_present: 0 },
  })))
check('an unexplained hold still names the bottleneck',
  holdExplanation({
    verdict: 'hold', psi: 0, sentience: 0, bottleneck: 'rationality', note: '', inputs: {},
  }).includes('rationality'))

// A HOLD no longer ends the turn — the state block is withheld and she is told why, so
// she can report the fault instead of the terminal going dark during it. These pin the
// four things that instruction has to do, because getting any one wrong reintroduces the
// failure the gate exists to prevent (or the uselessness the 409 caused).
const heldWedged = holdInstruction({
  verdict: 'hold', psi: 0, sentience: 0.4, bottleneck: 'logic', note: '',
  inputs: { metrics_age_secs: 59105, sniper_running: true, state_files_present: 4 },
})
check('a hold instruction carries the diagnosis, not just the verdict',
  /wedged|stale/i.test(heldWedged) && heldWedged.includes('985 minute'))
check('a hold instruction says she has no data at all',
  /no bot data|no SCEMATICA STATE block/i.test(heldWedged))
check('a hold instruction forbids citing figures',
  /cite no figure|do not cite/i.test(heldWedged))
// Without this she treats HOLD as a blanket refusal and stops answering questions that
// never needed the bot — which is the 409's failure mode reintroduced in prose.
check('a hold instruction still permits non-bot questions',
  /answer normally/i.test(heldWedged))
// The stale-history trap: the block is gone, but earlier turns in the conversation still
// contain figures, and quoting those back is indistinguishable from having live data.
check('a hold instruction rules out reusing figures from earlier turns',
  /earlier in this conversation|previous turn/i.test(heldWedged))

const caution = cautionInstruction({
  verdict: 'caution', psi: 0.0819, sentience: 0.4, bottleneck: 'perception', note: '', inputs: {},
})
check('a caution instruction carries Ψ', caution.includes('0.082'))
check('a caution instruction names the bottleneck', caution.includes('perception'))

// Absent is not the same as HOLD: a deploy with no bot, or an API too old to have the
// endpoint, must stay fully usable rather than refuse every question.
check('an unreachable gate is no opinion, not a refusal',
  (await readGate('http://127.0.0.1:1')) === null)

console.log('\n\u2500\u2500 codex \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500')

// The whole value of the codex is that it was written from the repository rather than from
// the names, and the only thing that keeps that true over time is this: a renamed or deleted
// crate fails the build instead of quietly becoming folklore she recites with confidence.
const missingPaths = CODEX.filter((e) => e.path && !existsSync(join(REPO_ROOT, e.path)))
check(
  `every codex path exists on disk${missingPaths.length ? ` (missing: ${missingPaths.map((e) => e.path).join(', ')})` : ''}`,
  missingPaths.length === 0,
)

const ids = codexIds()
check('codex ids are unique', new Set(ids).size === ids.length)

// A dangling `related` id renders as a broken cross-reference in a tool result, which the
// model reads as a real area and then describes. Same failure class as a missing path.
const dangling = CODEX.flatMap((e) =>
  e.related.filter((r) => !codexEntry(r)).map((r) => `${e.id}->${r}`),
)
check(`every related id resolves${dangling.length ? ` (dangling: ${dangling.join(', ')})` : ''}`,
  dangling.length === 0)

check('every entry has a summary', CODEX.every((e) => e.summary.trim().length > 30))
check('the codex covers Scematica Omni', codexEntry('scematica-omni') !== null)
check('the codex covers the omni daemon Scylar actually calls', codexEntry('scema-daemon') !== null)
check('the codex covers Scylar herself', codexEntry('scylar') !== null)
check('the codex covers the psyche', codexEntry('scylar-psyche') !== null)

// Search has to find the daemon from how an operator would ask for it, not only from its id.
check('search finds the daemon from a sentence',
  searchCodex('how does the omni daemon authenticate').some((e) => e.id === 'scema-daemon'))
check('search finds the coherence breaker by concept',
  searchCodex('epistemic breaker fail open').some((e) => e.id === 'coherence-breaker'))
check('search finds the vault from "proof of reserve"',
  searchCodex('proof of reserve').some((e) => e.id === 'escrow' || e.id === 'scematica-vault'))

// The refusal is the point: a miss must not be answered from the name. This is the one
// codex behaviour that, if it broke, would produce confident invention with no tell.
const miss = lookup('quantum flux capacitor subsystem')
check('a codex miss says so plainly', /has no entry/.test(miss))
check('a codex miss offers the real areas instead', miss.includes('Bot crates:'))

check('the codex map groups by kind', codexMap().includes('Omni crates:'))
check('the codex map lists every id',
  ids.every((id) => codexMap().includes(id)))

console.log('\n\u2500\u2500 psyche \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500')

const baseCtx = {
  turn: 0,
  utterance: 'what can you see right now?',
  senses: { bot: 'live', omni: true, sealing: false, codex: true, tools: ['explain_project'] },
  gate: { verdict: 'go', psi: 0.91, bottleneck: null },
  provider: { id: 'groq', model: 'llama-3.3-70b-versatile' },
  blocks: {},
  budget: DEFAULT_BUDGET,
}

const composed = composePsyche(baseCtx)

// The identity line the whole overhaul is about. Asserted verbatim: it is the one string a
// well-meaning edit is most likely to "improve" into something that no longer says what she
// is, and it is the first thing every turn is conditioned on.
check('the identity layer states who she is, verbatim',
  composed.text.startsWith('I am Scylar, the Scematica sentience AI assistant.'))

check('identity comes before epistemics',
  composed.active.indexOf('identity-core') < composed.active.indexOf('epistemic-core'))

// Composition is pure: same context, same string. Without this nothing else here can be
// pinned, and a turn that went wrong could not be reproduced from its header.
check('composition is deterministic', composePsyche(baseCtx).text === composed.text)

// The two layers that may never be budgeted out. The failure they prevent — a confident
// fabrication in her own voice — is the one nobody can see from outside the process.
const starved = composePsyche({ ...baseCtx, budget: 1 })
check('identity survives a zero budget', starved.active.includes('identity-core'))
check('epistemics survives a zero budget', starved.active.includes('epistemic-core'))
check('a starved budget drops something rather than silently fitting',
  starved.dropped.length > 0)

// Interoception is the layer that makes "what can you see?" answerable rather than guessed.
// Each bot state has to reach the prompt as a DIFFERENT sentence, because they are four
// different next actions for the operator.
const senseText = (bot) =>
  composePsyche({ ...baseCtx, senses: { ...baseCtx.senses, bot } }).text
check('a live bot reads as open', /Bot: OPEN/.test(senseText('live')))
check('a toggled-off bot is not reported as a fault', /Bot: OFF/.test(senseText('off')))
check('an unreachable bot is dark', /Bot: DARK/.test(senseText('unavailable')))
check('a withheld bot is distinguished from an absent one',
  /Bot: WITHHELD/.test(senseText('held')) && !/Bot: DARK/.test(senseText('held')))
check('a simulated bot is labelled as simulated', /Bot: SIMULATED/.test(senseText('simulation')))

// Ψ has to arrive as a number when measured and as the word when it is not — the same
// distinction the gauge draws, stated in the one place the model reads.
check('interoception carries a measured Ψ', composed.text.includes('0.91'))
check('interoception says unmeasured rather than 0.00',
  /\u03a8 unmeasured/.test(
    composePsyche({ ...baseCtx, gate: { verdict: 'go', psi: null, bottleneck: null } }).text,
  ))

// Continuity is about *this* session, so it has nothing to say on the first turn and must
// not claim a memory she does not have on any turn.
check('continuity is absent on the first turn', !composed.active.includes('continuity'))
check('continuity applies once there is history',
  composePsyche({ ...baseCtx, turn: 3 }).active.includes('continuity'))
check('first contact applies only on the first turn',
  composed.active.includes('first-contact') &&
    !composePsyche({ ...baseCtx, turn: 1 }).active.includes('first-contact'))

// The self-model has to state the limit rather than leave it to be inferred: a model asked
// whether it is conscious will otherwise overclaim or deflect, and both are worse than the
// true answer. This is the layer that keeps a "sentience assistant" honest about the word.
check('the self-model refuses to claim inner experience',
  /not\s+a claim about inner experience/i.test(composed.text))
check('the self-model names her own modules',
  composed.text.includes('psyche.ts') && composed.text.includes('codex.ts'))

// Situational blocks travel with their data and sit at the end — a rule stated beside the
// thing it governs survives, one stated in a preamble is averaged away.
const withBlocks = composePsyche({
  ...baseCtx,
  blocks: { botState: 'SCEMATICA STATE\npnl: +1.2', gateInstruction: 'GATE: caution' },
})
check('the state block lands after the identity',
  withBlocks.text.indexOf('SCEMATICA STATE') > withBlocks.text.indexOf('I am Scylar'))
check('a gate instruction survives a zero budget',
  composePsyche({
    ...baseCtx, budget: 1, blocks: { gateInstruction: 'GATE: caution' },
  }).active.includes('gate-instruction'))

// The budget exists so a future layer cannot silently push the state block out — not to
// trim the present set. This is what turns that intent into something that fails the build:
// every layer applicable, a live state block, a CAUTION gate, the tool list and the full
// omni doctrine, and nothing may drop.
const fullLoad = composePsyche({
  ...baseCtx,
  turn: 4,
  senses: { ...baseCtx.senses, sealing: true },
  gate: { verdict: 'caution', psi: 0.42, bottleneck: 'perception' },
  blocks: {
    botState: `SCEMATICA STATE (LIVE)
${'x'.repeat(1400)}`,
    gateNote: 'note '.repeat(20),
    gateInstruction: 'GATE: caution '.repeat(20),
    toolInstruction: 'tools '.repeat(60),
    omniInstruction: OMNI_INSTRUCTION(['omni_simulate', 'omni_records', 'omni_verify'], true),
  },
})
check(`a full-load turn drops nothing (${fullLoad.chars}/${DEFAULT_BUDGET} chars)`,
  fullLoad.dropped.length === 0)
check('a full-load turn still carries the codex map',
  fullLoad.active.includes('codex-map') && fullLoad.active.includes('self-model'))

check('every registered layer has a unique id',
  new Set(PSYCHE.map((i) => i.id)).size === PSYCHE.length)
check('the header reports what was injected',
  composed.active.length > 0 && composed.active.every((id) => PSYCHE.some((i) => i.id === id)))

console.log('\n\u2500\u2500 sigil \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500')

// 0 degrees is twelve o'clock. Absorbed in `polar` so no call site has to think about SVG's
// three-o'clock origin, and wrong here means every arc is rotated a quarter turn.
const top = polar(50, 0)
check('0 degrees is the top', Math.abs(top.x - 100) < 0.001 && top.y < 100)
const right = polar(50, 90)
check('90 degrees is the right', right.x > 100 && Math.abs(right.y - 100) < 0.001)

// THE rule of this file, in its two halves. An unmeasured gauge and a measured zero must
// not produce the same picture — that is the em-dash failure in vector form.
const ghost = gaugeArc(null, 78, -135, 135)
const zero = gaugeArc(0, 78, -135, 135)
const half = gaugeArc(0.5, 78, -135, 135)
check('an unmeasured gauge is not measured', ghost.measured === false)
check('an unmeasured gauge reads as an em dash', ghost.label === '\u2014')
check('a measured zero reads as 0.00', zero.label === '0.00' && zero.measured === true)
check('an unmeasured gauge draws the full sweep, not nothing', ghost.d === ghost.track)
check('a measured zero draws no arc', zero.d === '')
check('unmeasured and measured-zero are visibly different', ghost.d !== zero.d)
check('only an unmeasured gauge is ghosted', ghost.ghost === true && zero.ghost === false)
check('a half gauge is shorter than the full track',
  half.d.length > 0 && half.d !== half.track)
check('a gauge clamps above 1 rather than wrapping past its own start',
  gaugeArc(1.4, 78, -135, 135).d === gaugeArc(1, 78, -135, 135).d)
check('a gauge clamps below 0', gaugeArc(-3, 78, -135, 135).label === '0.00')
check('a NaN gauge is unmeasured, not zero', gaugeArc(NaN, 78, -135, 135).label === '\u2014')

// A degenerate arc renders as nothing on some engines and as a full circle on others, and
// a full circle is the single worst thing a zero gauge could draw.
check('a zero-sweep arc is empty, never a full circle', arcPath(50, 90, 90) === '')
check('a full sweep is drawn as two arcs', (arcPath(50, 0, 360).match(/A /g) || []).length === 2)

// Coverage: one cell per term, never a proportional bar. A bar renders 2/5 and 4/10
// identically, and the denominator is the number that matters.
const cov = coverageCells(2, 9)
check('coverage is one cell per term', cov.cells.length === 9)
check('coverage fills only the measured cells',
  cov.cells.filter(Boolean).length === 2 && cov.label === '2/9')
check('2/5 and 4/10 are different meters',
  coverageCells(2, 5).cells.length !== coverageCells(4, 10).cells.length)
check('an absent coverage is null, not an empty meter', coverageCells(0, 0) === null)

// Colour is a claim about trust, decided in one place. `held` must not collapse into
// `dark`: a channel that read fine and was withheld is a different fact from one that
// never answered, and only one of them is a fault.
check('an open channel reads live', channelRole('open') === 'live')
check('held is its own role, not dark', channelRole('held') !== channelRole('dark'))
check('simulated is its own role', channelRole('simulated') === 'sim')

// Motion is a claim, not decoration. An idle ring must be the slowest thing on the page and
// must not pulse, or a stopped stream is indistinguishable from a running one.
const mIdle = motionFor({ kind: 'idle' })
const mThink = motionFor({ kind: 'thinking' })
const mSpeak = motionFor({ kind: 'streaming', elapsedMs: 0 })
check('idle is the slowest rotation', mIdle.spinSecs > mThink.spinSecs && mIdle.spinSecs > mSpeak.spinSecs)
check('an idle ring does not pulse', mIdle.pulse === false && mThink.pulse === true)
check('the counter-ring is slower than the ring it opposes',
  mIdle.counterSecs > mIdle.spinSecs && mSpeak.counterSecs > mSpeak.spinSecs)
check('intensity rises from idle to speaking', mSpeak.intensity > mIdle.intensity)

// An idle trace that wiggles is a fabricated readout.
const flat = tracePoints([], 84, 18)
check('an empty trace is a flat line', flat.split(' ').length === 2 && flat.includes('9'))
const live = tracePoints([1, 4, 2], 84, 18, 8)
check('a live trace fills every slot', live.split(' ').length === 8)
check('a short history pads flat on the left rather than stretching',
  live.split(' ')[0].endsWith(',9'))

// The status word: a HOLD outranks whatever the phase is doing, because it is a statement
// about whether the answer being streamed can be trusted at all.
const chans = [{ id: 'bot', label: 'BOT', state: 'dark', title: '' }]
const viewHeld = sigilView({
  phase: { kind: 'streaming', elapsedMs: 0 }, psi: 0.2, verdict: 'hold',
  coverage: null, channels: chans, trace: [],
})
check('a hold outranks the phase in the status word', viewHeld.status === 'HELD')
check('an all-dark ring says OFFLINE',
  sigilView({
    phase: { kind: 'idle' }, psi: null, verdict: null, coverage: null, channels: chans, trace: [],
  }).status === 'OFFLINE')
check('an absent coverage survives to the view as null',
  sigilView({
    phase: { kind: 'idle' }, psi: null, verdict: null, coverage: null, channels: chans, trace: [],
  }).coverage === null)

// Ticks and radii are shared constants so the component and these checks cannot disagree.
// Layout collisions. The readouts stack in the column the Ψ arc leaves open at the bottom,
// and a channel node landing in that column draws a two-letter label straight through a
// number. Both are geometry, so both are checkable — and neither is visible in a diff.
const bands = [READOUT.trace.y + READOUT.trace.h / 2, READOUT.coverage.y, READOUT.status.y]
check('the readout bands stack without overlapping',
  bands.every((y, i) => i === 0 || y > bands[i - 1]))
check('the readout stack fits inside the viewBox', READOUT.status.y < 200)
check('the Ψ figure sits above the portrait, clear of the stack', READOUT.psi.y < READOUT.trace.y)

// Four channels on the diagonals. `Math.abs(x - CENTER) > half the trace width` is the
// actual property: no node may sit in the column the trace, meter and status occupy.
const nodes = channelPositions(4, RADIUS.channels, CHANNEL_START)
check('no channel node sits in the readout column',
  nodes.every((p) => Math.abs(p.x - CENTER) > READOUT.trace.w / 2 || p.y < READOUT.trace.y - 20))
check('channel nodes are evenly spread', new Set(nodes.map((p) => Math.round(p.x))).size === 2)

check('ticks are evenly spaced and marked', ticks(60, RADIUS.ticks).filter((t) => t.major).length === 12)
check('the gauge sits inside the tick ring', RADIUS.gauge < RADIUS.ticks)
check('the channel nodes sit inside the gauge', RADIUS.channels < RADIUS.gauge)

console.log('\n\u2500\u2500 codex tools \u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500')

const codexOnly = { bot: false, omni: false, seal: false, codex: true }

// The point of the codex group: a deployment with no bot and no daemon can still explain
// the project. Before it existed, a stopped sniper meant she could not answer a question
// about the repository — the exact moment an operator most wants one.
check('the codex is available with nothing else running',
  availableTools(codexOnly).length === 2)
check('a codex-only deployment offers no bot or omni tools',
  availableTools(codexOnly).every((t) => t.group === 'codex'))

// A local tool has no path at all, which is stronger than hard-coding one: there is no URL
// for a model to aim at, and no way for a later edit to give the codex one by accident.
check('codex tools reach no URL',
  TOOLS.filter((t) => t.group === 'codex').every((t) => !t.path && typeof t.local === 'function'))
check('every non-local tool still has a path',
  TOOLS.filter((t) => !t.local).every((t) => typeof t.path === 'string' && t.path.length > 0))

const explained = await runTool('http://127.0.0.1:1', 'explain_project', '{"topic":"scema-daemon"}', codexOnly)
check('explain_project answers without a network', explained.ok === true)
check('explain_project returns the invariants, not just a summary',
  explained.content.includes('Invariants'))
check('explain_project reaches Scematica Omni',
  explained.content.includes('loopback'))

const missed = await runTool('http://127.0.0.1:1', 'explain_project', '{"topic":"flux capacitor"}', codexOnly)
check('an unknown topic is refused rather than invented',
  missed.ok === true && /has no entry/.test(missed.content))

const areas = await runTool('http://127.0.0.1:1', 'list_project_areas', '{}', codexOnly)
check('list_project_areas returns the map', areas.ok === true && areas.content.includes('Web products:'))

console.log(`\n${failed === 0 ? 'ALL PASS' : `${failed} FAILED`}`)
process.exit(failed === 0 ? 0 : 1)
