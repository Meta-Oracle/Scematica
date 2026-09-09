// Every product on scematica.org, once.
//
// This table is the single source for the desktop chip row, the mobile menu and the
// sitemap. Before it existed the chips were eight hand-written `<Link>` blocks and the
// sitemap was a separate list, so adding a product meant remembering two places and
// getting it wrong meant a page that existed and could not be found — which is the
// quieter half of the same failure as a link that overflows off-screen.
//
// ── Why this lives in components/ and not lib/ ───────────────────────────────
//
// `tailwind.config.ts` scans `pages/`, `components/` and `app/`. It does **not** scan
// `lib/`. The `chip` strings below are Tailwind classes, so a table in `lib/` would have
// every one of them purged from the production stylesheet — the chips would render with
// no colour at all, in the built site only, while `npm run dev` looked perfect. The
// palettes are the one thing distinguishing eight products at a glance, so this is not a
// cosmetic risk.

export interface Product {
  href: string
  /** Shown on the chip and in the menu. */
  label: string
  /** One glyph, distinct per product — the chips are read by shape before colour. */
  glyph: string
  /** One line, for the mobile menu. A chip has room for a name; a menu row does not
   *  have to guess what the name means. */
  blurb: string
  /** Tailwind classes for the chip and the menu row. Literal strings, see the note above. */
  chip: string
  /** Sitemap: how often the page is worth re-reading. */
  changeFrequency: 'hourly' | 'daily' | 'weekly'
  priority: number
}

export const PRODUCTS: Product[] = [
  {
    href: '/alchem-link',
    label: 'ALCHEM-LINK',
    glyph: '◈',
    blurb: 'Chainlink oracle console — live feeds, measured staleness, consumer-safety audits',
    chip: 'border-alchem-border text-alchem-blue hover:border-alchem-blue hover:text-alchem-blue-hi hover:shadow-blue-sm',
    changeFrequency: 'daily',
    priority: 0.8,
  },
  {
    href: '/botchain',
    label: 'BOT CHAIN',
    glyph: '⬢',
    blurb: 'The EVM port — chain 677, verifiable inference',
    chip: 'border-botchain-border text-botchain-amber hover:border-botchain-amber hover:text-botchain-amber-hi',
    changeFrequency: 'weekly',
    priority: 0.6,
  },
  {
    href: '/scylar-terminal',
    label: 'SCYLAR',
    glyph: '◈',
    blurb: 'The sentience assistant over the bot, the loop and this repository',
    chip: 'border-scylar-border text-scylar-violet hover:border-scylar-violet hover:text-scylar-violet-hi',
    changeFrequency: 'weekly',
    priority: 0.7,
  },
  {
    href: '/escrow',
    label: 'ESCROW',
    glyph: '⬡',
    blurb: 'Proof of reserve — is the money actually there?',
    chip: 'border-escrow-border text-escrow-teal hover:border-escrow-teal hover:text-escrow-teal-hi',
    changeFrequency: 'daily',
    priority: 0.8,
  },
  {
    href: '/mesh',
    label: 'MESH',
    glyph: '◇',
    blurb: "The running system's own topology, as a live graph",
    chip: 'border-mesh-border text-mesh-accent hover:border-mesh-accent hover:text-mesh-glow',
    changeFrequency: 'hourly',
    priority: 0.7,
  },
  {
    href: '/omni',
    label: 'OMNI',
    glyph: '◆',
    blurb: 'Verify a sealed decision record, offline, with no server in the path',
    chip: 'border-omni-border text-omni-accent hover:border-omni-accent hover:text-omni-glow',
    changeFrequency: 'weekly',
    priority: 0.9,
  },
  {
    href: '/scema-world',
    label: 'SCEMA-WORLD',
    glyph: '✦',
    blurb: 'Fly a decision record — the map IS the record',
    chip: 'border-sw-border text-sw-accent hover:border-sw-accent hover:text-sw-glow',
    changeFrequency: 'weekly',
    priority: 0.9,
  },
  {
    href: '/zero',
    label: 'ZERO',
    glyph: '○',
    blurb: 'The loop in your browser — no install, no local API, no custody',
    chip: 'border-zero-border text-zero-accent hover:border-zero-accent hover:text-zero-text',
    changeFrequency: 'weekly',
    priority: 0.9,
  },
]
