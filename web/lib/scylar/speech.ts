// Speech: turning a written answer into something worth listening to, and into mouth
// movement that matches it.
//
// The payoff is not "she can talk" — it is that `SpeechSynthesisUtterance` fires an
// `onboundary` event per word, so the flap can be driven by *actual speech timing*
// instead of a fixed 180ms cycle. A timer-driven mouth is a loop that happens to be
// running while text appears; a boundary-driven one opens on the word. That is the whole
// difference between an animated portrait and one that is speaking.
//
// Everything here is pure and browser-free so it can be tested without a speech engine.
// The engine itself lives in `components/scylar/useSpeech.ts`.

/**
 * Strip markdown down to what should be read aloud.
 *
 * Reading punctuation is the fastest way to make a voice feel mechanical: "star star
 * bold star star", or forty seconds of a Rust function spelled out symbol by symbol.
 * Code blocks are replaced by a spoken summary rather than dropped silently — the
 * listener needs to know something was skipped, or the answer sounds like it has a hole
 * in it, and the block is still on screen to read.
 */
export function speakableText(markdown: string): string {
  let text = markdown

  // Fenced code → a spoken stand-in. Handles an unterminated fence too, which is what a
  // stream that was stopped mid-block leaves behind.
  text = text.replace(/```(\w*)\n?([\s\S]*?)(?:```|$)/g, (_m, lang: string) =>
    lang ? ` (${lang} code block) ` : ' (code block) ',
  )

  text = text
    // Links: keep the label, drop the URL. Nobody wants an https read to them.
    .replace(/\[([^\]\n]+)\]\([^)\s]+\)/g, '$1')
    .replace(/`([^`\n]+)`/g, '$1')
    .replace(/\*\*([^*]+)\*\*/g, '$1')
    .replace(/^#{1,6}\s+/gm, '')
    // List markers become a pause, so items don't run together into one sentence.
    .replace(/^\s*[-*]\s+/gm, '')
    .replace(/^\s*(\d+)[.)]\s+/gm, '$1. ')
    // A run of separators reads as a long silence otherwise.
    .replace(/^[-=_]{3,}$/gm, '')
    .replace(/[ \t]+/g, ' ')
    .replace(/\n{2,}/g, '\n')
    .trim()

  return text
}

/** Longest chunk handed to one utterance. */
const MAX_CHUNK_CHARS = 200

/**
 * Split into utterance-sized pieces at sentence boundaries.
 *
 * Not an optimisation. Chrome stops speaking after roughly 15 seconds of a single
 * utterance and fires no error when it does, so a long answer simply goes quiet
 * mid-sentence. Splitting keeps each utterance short enough to finish, and gives the
 * queue natural places to stop when the operator interrupts.
 */
export function splitForSpeech(text: string): string[] {
  if (!text.trim()) return []

  const sentences = text.match(/[^.!?\n]+[.!?]*\s*|\n+/g) ?? [text]
  const chunks: string[] = []
  let current = ''

  for (const raw of sentences) {
    const piece = raw.replace(/\n+/g, ' ')
    if (!piece.trim()) continue

    if (current.length + piece.length > MAX_CHUNK_CHARS && current) {
      chunks.push(current.trim())
      current = piece
    } else {
      current += piece
    }

    // A single sentence longer than the cap still has to be broken, or it inherits the
    // silent-cutoff problem the split exists to avoid.
    while (current.length > MAX_CHUNK_CHARS) {
      const cut = current.lastIndexOf(' ', MAX_CHUNK_CHARS)
      const at = cut > MAX_CHUNK_CHARS / 2 ? cut : MAX_CHUNK_CHARS
      chunks.push(current.slice(0, at).trim())
      current = current.slice(at)
    }
  }

  if (current.trim()) chunks.push(current.trim())
  return chunks.filter(Boolean)
}

/** Bounds on a spoken word, in ms. Outside these the mouth stops matching the audio. */
const MIN_WORD_MS = 130
const MAX_WORD_MS = 620

/**
 * Estimate how long a word will take to say.
 *
 * `onboundary` reports where a word *starts*, never how long it lasts, and the next
 * boundary arrives only once the word is already over — too late to open the mouth for
 * it. So the duration is predicted from length and speaking rate. Being slightly wrong
 * is invisible; the mouth is closed for the tail of a word either way, which is what
 * speech looks like.
 */
export function estimateWordMs(word: string, rate = 1): number {
  const chars = Math.max(1, word.trim().length)
  const ms = (chars * 58 + 90) / Math.max(0.5, rate)
  return Math.round(Math.min(MAX_WORD_MS, Math.max(MIN_WORD_MS, ms)))
}

/** The word starting at `charIndex`, used to size the flap for it. */
export function wordAt(text: string, charIndex: number): string {
  if (charIndex < 0 || charIndex >= text.length) return ''
  const end = text.indexOf(' ', charIndex)
  return text.slice(charIndex, end === -1 ? undefined : end)
}

/** True when this browser can speak. Safari and Chrome yes; some webviews no. */
export function speechSupported(): boolean {
  return typeof window !== 'undefined' && 'speechSynthesis' in window
}

/**
 * Known female English voice names, by first name.
 *
 * `SpeechSynthesisVoice` exposes `name`, `lang`, `voiceURI`, `localService` and
 * `default` — and no gender field at all. Since Scylar is a specific character rather
 * than a generic reader, the only way to honour that is a name table. It is matched on
 * whole tokens, never substrings: voice names are long strings like "Microsoft Zira
 * Desktop - English (United States)", and substring matching on three-letter names finds
 * them inside unrelated words.
 *
 * Covers Windows SAPI and the newer Online (Natural) set, macOS/iOS, and Chrome/Android.
 * Unknown platforms fall through to the heuristics below rather than guessing.
 */
const FEMALE_VOICE_NAMES = new Set([
  // Windows — SAPI
  'zira', 'eva', 'hazel', 'susan', 'linda',
  // Windows — Online (Natural)
  'aria', 'ava', 'emma', 'jenny', 'michelle', 'monica', 'nova', 'sara', 'sarah',
  'ana', 'amber', 'ashley', 'cora', 'elizabeth', 'jane', 'nancy', 'sonia', 'libby',
  'maisie', 'natasha', 'clara', 'molly', 'luna', 'aideen', 'yan',
  // macOS / iOS
  'samantha', 'victoria', 'karen', 'moira', 'tessa', 'fiona', 'allison', 'zoe',
  'nicky', 'kathy', 'serena', 'martha', 'catherine', 'shelley', 'sandy',
])

/** Markers for the higher-quality synthesis engines, used only as a tiebreak. */
const QUALITY_MARKERS = ['natural', 'neural', 'online', 'premium', 'enhanced']

function tokens(name: string): string[] {
  return name.toLowerCase().split(/[^a-z]+/).filter(Boolean)
}

function isFemale(v: SpeechSynthesisVoice): boolean {
  const t = tokens(v.name)
  // Some Android and Chrome voices say so outright ("English United States female").
  if (t.includes('female')) return true
  if (t.includes('male')) return false
  return t.some((word) => FEMALE_VOICE_NAMES.has(word))
}

function hasQuality(v: SpeechSynthesisVoice): boolean {
  const lower = v.name.toLowerCase()
  return QUALITY_MARKERS.some((m) => lower.includes(m))
}

/**
 * Pick a voice for Scylar.
 *
 * **Gender is the primary key, quality only a tiebreak**, and the order matters more
 * than it looks. Ranking quality first — which is what this did originally — picks
 * "Microsoft Andrew Online (Natural)" over "Microsoft Zira" on a stock Windows install,
 * because Edge lists the male natural voice and `natural` matched before any name did.
 * The result was a male voice for a character who is not one, which no amount of good
 * synthesis makes right.
 *
 * Everything degrades: an unknown platform with no recognisable female voice falls back
 * to the platform default rather than refusing to speak.
 */
export function pickVoice(voices: SpeechSynthesisVoice[]): SpeechSynthesisVoice | null {
  if (!voices.length) return null
  const english = voices.filter((v) => v.lang?.toLowerCase().startsWith('en'))
  const pool = english.length ? english : voices

  const female = pool.filter(isFemale)
  if (female.length) {
    return female.find(hasQuality) ?? female.find((v) => v.default) ?? female[0]
  }

  // No voice we can identify as female. Prefer quality, then the default — and the
  // caller can tell the operator that gender selection failed, via `pickedFemaleVoice`.
  return pool.find(hasQuality) ?? pool.find((v) => v.default) ?? pool[0]
}

/** Whether the chosen voice was actually identifiable as female. */
export function pickedFemaleVoice(voice: SpeechSynthesisVoice | null): boolean {
  return voice ? isFemale(voice) : false
}

/**
 * Delivery: stern rather than bright.
 *
 * Pitch does the work — below 1.0 it drops the register, and a raised pitch is what made
 * the first pass read as chirpy. The slight slow-down is the other half: sternness is
 * measured, not clipped. These are the only two levers `SpeechSynthesisUtterance`
 * offers; anything more comes from her word choice, which is the persona's job.
 */
export const VOICE_PITCH = 0.85
export const VOICE_RATE = 0.97
