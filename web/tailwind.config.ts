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
