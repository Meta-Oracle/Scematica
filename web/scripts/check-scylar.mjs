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
import { parseCommand } from '../lib/scylar/commands.ts'
import { deserialise, serialise } from '../lib/scylar/session.ts'
import { contextSystemMessage } from '../lib/scylar/context.ts'

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

console.log(`\n${failed === 0 ? 'ALL PASS' : `${failed} FAILED`}`)
process.exit(failed === 0 ? 0 : 1)
