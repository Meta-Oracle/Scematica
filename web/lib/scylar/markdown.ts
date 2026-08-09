// A very small markdown subset, parsed to a tree the terminal renders as React nodes.
//
// Written rather than installed for three reasons, in order of weight:
//
//   1. **It never produces HTML.** The output is data; the renderer builds elements. A
//      library that hands back an HTML string needs `dangerouslySetInnerHTML`, and the
//      string in question is attacker-adjacent — it comes from a model that will happily
//      repeat whatever a user pasted into the box.
//   2. **It has to survive being half-written.** Tokens arrive one at a time, so the
//      parser sees `` ```rust\nfn ma `` far more often than it sees a closed fence. An
//      unterminated fence is treated as a code block in progress, not as literal text
//      that snaps into a block when the closing fence lands.
//   3. The subset a chat model actually emits is small, and `web/` already avoids
//      dependencies it can spell out (see `lib/alchem/` on the same principle).
//
// Deliberately absent: tables, blockquotes, images, nested lists, HTML passthrough.
// Add them when something emits them, not before.

export interface InlineText {
  kind: 'text' | 'code' | 'strong'
  text: string
}

export interface InlineLink {
  kind: 'link'
  text: string
  href: string
}

export type Inline = InlineText | InlineLink

export type Block =
  | { kind: 'code'; lang: string; text: string; /** Fence not yet closed. */ open: boolean }
  | { kind: 'heading'; level: 1 | 2 | 3; inline: Inline[] }
  | { kind: 'list'; ordered: boolean; items: Inline[][] }
  | { kind: 'para'; inline: Inline[] }

const FENCE = /^\s*```([\w+-]*)\s*$/
const HEADING = /^(#{1,3})\s+(.*)$/
const BULLET = /^\s*[-*]\s+(.+)$/
const ORDERED = /^\s*\d+[.)]\s+(.+)$/

/**
 * Schemes a link may use.
 *
 * Everything else — `javascript:` above all — renders as plain text with the URL
 * visible. The model is not trusted here: it echoes user input, and a chat transcript is
 * exactly where a crafted link arrives.
 */
const SAFE_SCHEME = /^(https?:\/\/|mailto:|\/)/i

export function parseMarkdown(src: string): Block[] {
  const lines = src.split('\n')
  const blocks: Block[] = []

  let i = 0
  let para: string[] = []
  let list: { ordered: boolean; items: string[] } | null = null

  const flushPara = () => {
    if (para.length) {
      blocks.push({ kind: 'para', inline: parseInline(para.join('\n')) })
      para = []
    }
  }
  const flushList = () => {
    if (list) {
      blocks.push({
        kind: 'list',
        ordered: list.ordered,
        items: list.items.map(parseInline),
      })
      list = null
    }
  }
  const flush = () => {
    flushPara()
    flushList()
  }

  while (i < lines.length) {
    const line = lines[i]
    const fence = FENCE.exec(line)

    if (fence) {
      flush()
      const lang = fence[1] || ''
      const body: string[] = []
      i++
      let closed = false
      while (i < lines.length) {
        if (FENCE.test(lines[i])) {
          closed = true
          i++
          break
        }
        body.push(lines[i])
        i++
      }
      blocks.push({ kind: 'code', lang, text: body.join('\n'), open: !closed })
      continue
    }

    const heading = HEADING.exec(line)
    if (heading) {
      flush()
      blocks.push({
        kind: 'heading',
        level: heading[1].length as 1 | 2 | 3,
        inline: parseInline(heading[2]),
      })
      i++
      continue
    }

    const bullet = BULLET.exec(line)
    const ordered = ORDERED.exec(line)
    if (bullet || ordered) {
      flushPara()
      const isOrdered = Boolean(ordered)
      // A change of marker starts a new list rather than continuing the old one, so
      // bullets following a numbered list don't inherit its numbering.
      if (list && list.ordered !== isOrdered) flushList()
      list ??= { ordered: isOrdered, items: [] }
      list.items.push((bullet?.[1] ?? ordered?.[1] ?? '').trim())
      i++
      continue
    }

    if (line.trim() === '') {
      flush()
      i++
      continue
    }

    flushList()
    para.push(line)
    i++
  }

  flush()
  return blocks
}

/**
 * Inline spans, code first.
 *
 * Precedence is not cosmetic: a backtick span suppresses everything inside it, so
 * `` `**x**` `` is a literal asterisk pair and not bold text. Splitting on code before
 * looking for emphasis is what gets that right, and it matters here because half the
 * conversation is about code.
 */
export function parseInline(src: string): Inline[] {
  const out: Inline[] = []

  for (const chunk of splitCode(src)) {
    if (chunk.kind === 'code') {
      out.push(chunk)
      continue
    }
    out.push(...parseEmphasis(chunk.text))
  }

  return out.filter((n) => n.kind !== 'text' || n.text.length > 0)
}

function splitCode(src: string): Inline[] {
  const out: Inline[] = []
  const re = /`([^`\n]+)`/g
  let last = 0
  let m: RegExpExecArray | null

  while ((m = re.exec(src)) !== null) {
    if (m.index > last) out.push({ kind: 'text', text: src.slice(last, m.index) })
    out.push({ kind: 'code', text: m[1] })
    last = m.index + m[0].length
  }
  if (last < src.length) out.push({ kind: 'text', text: src.slice(last) })
  return out
}

function parseEmphasis(src: string): Inline[] {
  const out: Inline[] = []
  const re = /\*\*([^*]+)\*\*|\[([^\]\n]+)\]\(([^)\s]+)\)/g
  let last = 0
  let m: RegExpExecArray | null

  while ((m = re.exec(src)) !== null) {
    if (m.index > last) out.push({ kind: 'text', text: src.slice(last, m.index) })

    if (m[1] !== undefined) {
      out.push({ kind: 'strong', text: m[1] })
    } else {
      const [, , label, href] = m
      // Unsafe scheme: keep the text *and* show the URL. Dropping it silently would hide
      // that the model emitted a link at all, which is the thing worth seeing.
      out.push(
        SAFE_SCHEME.test(href)
          ? { kind: 'link', text: label, href }
          : { kind: 'text', text: `${label} (${href})` },
      )
    }
    last = m.index + m[0].length
  }
  if (last < src.length) out.push({ kind: 'text', text: src.slice(last) })
  return out
}
