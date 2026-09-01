import type { Config } from 'tailwindcss'

const config: Config = {
  content: [
    './pages/**/*.{js,ts,jsx,tsx,mdx}',
    './components/**/*.{js,ts,jsx,tsx,mdx}',
    './app/**/*.{js,ts,jsx,tsx,mdx}',
  ],
  theme: {
    extend: {
      colors: {
        'scema-black':   '#080808',
        'scema-dark':    '#0f0f0f',
        'scema-panel':   '#111111',
        'scema-border':  '#1a0000',
        'scema-red':     '#cc1111',
        'scema-red-hi':  '#ff2020',
        'scema-red-dim': '#661111',
        'scema-red-bg':  '#1a0000',
        'scema-text':    '#c8c8c8',
        'scema-muted':   '#666666',
        'scema-dim':     '#333333',
        'scema-green':   '#00cc44',
        'scema-amber':   '#ffaa00',

        // ── alchem-link ────────────────────────────────────────────────────
        // The /alchem-link page is a different product on the same site, so it
        // gets its own identity: black surfaces with a mid-blue signal, against
        // the sniper's black-and-red. These values mirror
        // `alchem-link/src/alchem_link/theme.py` exactly — the terminal and the
        // web build are meant to be recognisably the same tool, so when one
        // palette moves, move both.
        'alchem-black':   '#04070c',
        'alchem-surface': '#080e17',
        'alchem-hi':      '#0e1926',
        'alchem-border':  '#1b3d63',
        'alchem-border-hi':'#2c6296',
        'alchem-blue':    '#4d9fff',
        'alchem-blue-hi': '#7cc0ff',
        'alchem-blue-dim':'#2f6ba8',
        'alchem-text':    '#cddced',
        'alchem-muted':   '#7f9ec0',
        'alchem-dim':     '#47678c',
        'alchem-green':   '#2ee6a0',
        'alchem-amber':   '#ffb340',
        'alchem-red':     '#ff5c78',

        // ── scylar-terminal ────────────────────────────────────────────────
        // A third product on the same site, so a third identity: violet on
        // near-black, against alchem's blue and the sniper's red. These values
        // are sampled from the character art in `scylar-terminal/
        // scylar-expressions/` — the portrait sits inside the page, so a
        // palette that fights it reads as two designs stacked. If the art is
        // ever regenerated, resample rather than guessing.
        'scylar-black':   '#05030c',
        'scylar-surface': '#0c0818',
        'scylar-hi':      '#150e28',
        'scylar-border':  '#3a1f6b',
        'scylar-border-hi':'#5c34a3',
        'scylar-violet':  '#a970ff',
        'scylar-violet-hi':'#c9a3ff',
        'scylar-violet-dim':'#6f45b8',
        'scylar-text':    '#ded3f0',
        'scylar-muted':   '#a08cc4',
        'scylar-dim':     '#6b5a8e',
        'scylar-green':   '#2ee6a0',
        'scylar-red':     '#ff5c78',

        // ── botchain ────────────────────────────────────────────────────────
        // Fourth product, fourth identity: amber on near-black, against the
        // sniper's red, alchem's blue and scylar's violet. Picked to be
        // distinguishable at a glance rather than sampled from brand assets —
        // if BOT Chain publishes a palette, resample these rather than guess.
        'botchain-black':   '#0a0703',
        'botchain-surface': '#161009',
        'botchain-hi':      '#241a0d',
        'botchain-border':  '#5c421a',
        'botchain-border-hi':'#8a6526',
        'botchain-amber':   '#f0a437',
        'botchain-amber-hi':'#ffc46b',
        'botchain-amber-dim':'#a8742a',
        'botchain-text':    '#f0e4d2',
        'botchain-muted':   '#c4a882',
        'botchain-dim':     '#8e765a',
        'botchain-green':   '#3ddc97',
        'botchain-red':     '#ff6b5c',

        // ── mesh ────────────────────────────────────────────────────────────
        // Sixth product, sixth identity: indigo night, against the sniper's red,
        // alchem's blue, scylar's violet, botchain's amber and escrow's teal.
        //
        // The three status colours here are NOT decoration and must not be
        // reassigned for looks. They are the only thing separating "this unit is
        // reporting", "this unit last reported a while ago" and "this unit cannot
        // be seen at all" — the distinction the whole feature exists to preserve.
        // `mesh-absent` is deliberately close to the background: an unseen unit
        // should read as a hole in the picture, because that is what it is.
        'mesh-black':       '#050711',
        'mesh-surface':     '#0a0e1f',
        'mesh-hi':          '#131a33',
        'mesh-border':      '#1e2a4d',
        'mesh-border-hi':   '#354a7d',
        'mesh-text':        '#dfe6ff',
        'mesh-muted':       '#8f9cc7',
        'mesh-dim':         '#545f8a',
        'mesh-accent':      '#7c9cff',
        'mesh-glow':        '#52e5ff',
        'mesh-live':        '#4ade9b',
        'mesh-stale':       '#f5b544',
        'mesh-absent':      '#3a4266',
        'mesh-veto':        '#ff5d7d',

        // ── omni ────────────────────────────────────────────────────────────
        // Seventh product, seventh identity: warm amber on near-black, against
        // the sniper's red, alchem's blue, scylar's violet, botchain's amber,
        // escrow's teal and the mesh's indigo. Warmer and darker than
        // `botchain-amber` so the two are separable at a glance.
        //
        // `omni-unmeasured` is deliberately close to the surface colour, for the
        // same reason `mesh-absent` is close to the background: a term nobody
        // measured must not draw the eye like a value somebody did. It is the
        // colour of the em dash that stands in for a number, and it is the last
        // line of defence for everything the Rust type system protects.
        'omni-black':       '#0b0906',
        'omni-surface':     '#141009',
        'omni-hi':          '#1f180c',
        'omni-border':      '#33280f',
        'omni-border-hi':   '#5a4518',
        'omni-text':        '#f3e8d2',
        'omni-muted':       '#b09872',
        'omni-dim':         '#6f5f45',
        'omni-accent':      '#ffb340',
        'omni-glow':        '#ffd98a',
        'omni-valid':       '#5ddc9a',
        'omni-invalid':     '#ff6b6b',
        'omni-warn':        '#ffb86b',
        'omni-unmeasured':  '#463a26',

        // ── scema-world ─────────────────────────────────────────────────────
        // Eighth product, eighth identity: cold violet-white on true black, the
        // colour of a cockpit at night. Distinct from scylar's warm violet, which
        // is a face, and from omni's amber, which is a document — this is the
        // instrument glow of something you are flying.
        //
        // Two of these are not decoration and must not be tuned into each other:
        // `sw-ghost` marks a contact nobody measured, and `sw-rift` marks a lane
        // that ends because the observer could not see past it. Both are the
        // em-dash rule in the one place a player acts on it, and both are the
        // colour of *not knowing* rather than of a low value.
        'sw-black':      '#04030a',
        'sw-surface':    '#0b0918',
        'sw-border':     '#241f45',
        'sw-border-hi':  '#413a78',
        'sw-text':       '#e8e6ff',
        'sw-muted':      '#a09bd0',
        'sw-dim':        '#5f5a90',
        'sw-accent':     '#a96bff',
        'sw-glow':       '#d0aaff',
        'sw-hostile':    '#ff595f',
        'sw-salvage':    '#59e69e',
        'sw-ghost':      '#6f6690',
        'sw-rift':       '#4a4470',

        // ── escrow market ───────────────────────────────────────────────────
        // Fifth product, fifth identity: a cold teal on near-black, against the
        // sniper's red, alchem's blue, scylar's violet and botchain's amber.
        // Teal rather than gold on purpose — this is a vault page whose entire
        // claim is "the money is verifiably there and nobody can take it", and
        // gold reads as a yield pitch. The one warm colour in the set is
        // reserved for `escrow-alarm`, which marks a reserve shortfall and
        // should be the only thing on the page that ever looks urgent.
        'escrow-black':     '#03090a',
        'escrow-surface':   '#071316',
        'escrow-hi':        '#0c2126',
        'escrow-border':    '#154249',
        'escrow-border-hi': '#1f6b76',
        'escrow-teal':      '#2fd4c4',
        'escrow-teal-hi':   '#6ff0e3',
        'escrow-teal-dim':  '#1c8a80',
        'escrow-text':      '#d2eae8',
        'escrow-muted':     '#7fb3ae',
        'escrow-dim':       '#4a7b77',
        'escrow-locked':    '#2fd4c4',
        'escrow-alarm':     '#ff5c78',
      },
      fontFamily: {
        mono: ['Space Mono', 'JetBrains Mono', 'Fira Code', 'monospace'],
      },
      animation: {
        'cursor-blink': 'blink 1s step-end infinite',
        'glow-pulse':   'glow 2s ease-in-out infinite alternate',
        'scanline-scroll': 'scanScroll 8s linear infinite',
        'flicker':      'flicker 0.15s infinite',
        'text-glow':    'textGlow 3s ease-in-out infinite alternate',
        'fade-in':      'fadeIn 0.5s ease-in',
        'slide-in':     'slideIn 0.3s ease-out',
      },
      keyframes: {
        blink: {
          '0%, 100%': { opacity: '1' },
          '50%': { opacity: '0' },
        },
        glow: {
          '0%':   { boxShadow: '0 0 5px #cc1111, 0 0 10px #660000' },
          '100%': { boxShadow: '0 0 15px #ff2020, 0 0 30px #cc1111' },
        },
        scanScroll: {
          '0%':   { transform: 'translateY(0)' },
          '100%': { transform: 'translateY(4px)' },
        },
        flicker: {
          '0%, 19.999%, 22%, 62.999%, 64%, 64.999%, 70%, 100%': { opacity: '1' },
          '20%, 21.999%, 63%, 63.999%, 65%, 69.999%': { opacity: '0.85' },
        },
        textGlow: {
          '0%':   { textShadow: '0 0 4px #cc1111' },
          '100%': { textShadow: '0 0 12px #ff2020, 0 0 24px #cc1111' },
        },
        fadeIn: {
          '0%':   { opacity: '0' },
          '100%': { opacity: '1' },
        },
        slideIn: {
          '0%':   { transform: 'translateY(-8px)', opacity: '0' },
          '100%': { transform: 'translateY(0)', opacity: '1' },
        },
      },
      boxShadow: {
        'red-sm':  '0 0 6px rgba(204,17,17,0.4)',
        'red-md':  '0 0 12px rgba(204,17,17,0.5)',
        'red-lg':  '0 0 24px rgba(204,17,17,0.4), inset 0 0 24px rgba(102,0,0,0.1)',
        'panel':   'inset 0 1px 0 rgba(204,17,17,0.08)',
        'blue-sm': '0 0 6px rgba(77,159,255,0.35)',
        'blue-md': '0 0 12px rgba(77,159,255,0.45)',
        'blue-lg': '0 0 24px rgba(77,159,255,0.35), inset 0 0 24px rgba(27,61,99,0.25)',
      },
    },
  },
  plugins: [],
}
export default config
