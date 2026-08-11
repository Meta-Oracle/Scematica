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
import { TOOLS, runTool, toolDefinitions } from '../lib/scylar/tools.ts'
import {
  cautionInstruction,
  holdExplanation,
  holdInstruction,
  readGate,
} from '../lib/scylar/gate.ts'

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
check('every tool hard-codes its own path',
  TOOLS.every((t) => typeof t.path === 'string' && t.path.length > 0 && !t.path.includes('..')))
check('no tool targets a control route',
  TOOLS.every((t) => !t.path.startsWith('controls/') && !t.path.startsWith('push')))
// POST is allowed only for endpoints that compute and return. "Read-only" has to be a
// property of the list, not of the verb, or the first POST tool quietly widens it.
check('only the computing endpoint may POST',
  TOOLS.filter((t) => t.method === 'POST').every((t) => t.path === 'replay'))
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
  toolDefinitions().every((d) => d.type === 'function' && d.function.name && d.function.parameters))

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
const bogus = await runTool('http://127.0.0.1:1', 'get_everything', '{}')
check('an unknown tool is refused', bogus.ok === false)
check('an unknown tool names the real ones', bogus.content.includes('get_pool_decisions'))

// An unreachable bot is information, not an exception.
const unreachable = await runTool('http://127.0.0.1:1', 'get_controls', '{}')
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

console.log(`\n${failed === 0 ? 'ALL PASS' : `${failed} FAILED`}`)
process.exit(failed === 0 ? 0 : 1)
