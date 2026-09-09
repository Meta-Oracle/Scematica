#!/usr/bin/env node
// Pin the product nav.
//
// Two failures this prevents, both silent:
//
//   * a product in the table with no page — a menu entry that 404s;
//   * a page with no table entry — reachable only by somebody who already knows the URL,
//     which is the state the mobile nav was in for its whole life before this.
//
// And one that is silent *only in production*: the chip palettes are Tailwind classes in
// a `.ts` file, and `tailwind.config.ts` does not scan `lib/`. A table moved there would
// have every colour purged from the built stylesheet while `npm run dev` looked perfect.
//
//   node --experimental-strip-types scripts/check-nav.mjs

import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, join } from 'node:path'

import { PRODUCTS } from '../components/products.ts'

let failed = 0
const check = (name, ok) => {
  if (!ok) failed++
  console.log(`${ok ? 'PASS' : 'FAIL'}  ${name}`)
}

const HERE = dirname(fileURLToPath(import.meta.url))
const WEB = join(HERE, '..')

console.log('── the table ─────────────────────────────────────────────')

check('there are products at all', PRODUCTS.length > 0)
check('every href is unique', new Set(PRODUCTS.map(p => p.href)).size === PRODUCTS.length)
check('every label is unique', new Set(PRODUCTS.map(p => p.label)).size === PRODUCTS.length)
check('every href is site-absolute', PRODUCTS.every(p => p.href.startsWith('/') && !p.href.endsWith('/')))
// A menu row has space for a sentence and a chip does not; an empty blurb turns the
// mobile menu back into a list of names, which is what the chips already were.
check('every product says what it is', PRODUCTS.every(p => p.blurb.trim().length > 20))
check('every product has a glyph', PRODUCTS.every(p => p.glyph.trim().length > 0))
check('every priority is a sitemap priority', PRODUCTS.every(p => p.priority > 0 && p.priority <= 1))

console.log('\n── every product has a page ──────────────────────────────')

const missing = PRODUCTS.filter(p => {
  const dir = join(WEB, 'app', p.href.slice(1))
  return !existsSync(join(dir, 'page.tsx')) && !existsSync(join(dir, 'page.ts'))
})
check(
  `every product route resolves to an app page${missing.length ? ` — missing ${missing.map(p => p.href).join(', ')}` : ''}`,
  missing.length === 0,
)

console.log('\n── every page is in the nav ──────────────────────────────')

// Route groups, api and the dashboard itself are not products. Anything else with a
// page.tsx is a page a visitor can reach and should be able to find.
const NOT_PRODUCTS = new Set(['api', 'pair'])
const routes = readdirSync(join(WEB, 'app'), { withFileTypes: true })
  .filter(e => e.isDirectory() && !e.name.startsWith('(') && !e.name.startsWith('_'))
  .filter(e => !NOT_PRODUCTS.has(e.name))
  .filter(e => existsSync(join(WEB, 'app', e.name, 'page.tsx')))
  .map(e => `/${e.name}`)

const unlisted = routes.filter(r => !PRODUCTS.some(p => p.href === r))
check(
  `every reachable page is in the nav${unlisted.length ? ` — unlisted: ${unlisted.join(', ')}` : ''}`,
  unlisted.length === 0,
)

console.log('\n── one list, two surfaces ────────────────────────────────')

const nav = readFileSync(join(WEB, 'components', 'ProductNav.tsx'), 'utf8')
const page = readFileSync(join(WEB, 'app', 'page.tsx'), 'utf8')

// The whole point of the table. Hand-written links in the header are how the two
// surfaces drift, and a product present in one and absent from the other is unreachable
// on exactly one class of device.
const inlineProductLinks = PRODUCTS.filter(p => page.includes(`href="${p.href}"`))
check(
  `the header hand-writes no product links${inlineProductLinks.length ? ` — ${inlineProductLinks.map(p => p.href).join(', ')}` : ''}`,
  inlineProductLinks.length === 0,
)
check('the header renders ProductNav', page.includes('<ProductNav />'))

// The two surfaces, and the breakpoints that separate them.
check('the chip row is hidden below md', /hidden md:flex/.test(nav))
check('the disclosure is hidden at md and up', /md:hidden/.test(nav))
// Both must map over the same array, or they are two lists wearing one name.
check('both surfaces map over PRODUCTS', (nav.match(/PRODUCTS\.map/g) ?? []).length >= 2)

console.log('\n── the disclosure behaves ────────────────────────────────')

check('the button reports its state', /aria-expanded=\{open\}/.test(nav))
check('...and names the panel it controls', /aria-controls=\{panelId\}/.test(nav))
// A hamburger with no label is a settings icon on a trading dashboard.
check('the button says what it opens', /PRODUCTS · \$\{PRODUCTS\.length\}/.test(nav))
check('Escape closes it', /e\.key === 'Escape'/.test(nav))
// Without the focus return, dismissing leaves a keyboard user at the top of the document.
check('...and returns focus to the button', /buttonRef\.current\?\.focus\(\)/.test(nav))
check('a click outside closes it', /pointerdown/.test(nav))
check('choosing a product closes it', /onClick=\{close\}/.test(nav))
// A sticky header with a tall panel inside it must scroll the panel, not the page.
check('the panel scrolls rather than overflowing', /overflow-y-auto/.test(nav))
check('the panel is anchored to the header, full width', /left-0 right-0 top-full/.test(nav))

console.log('\n── the palettes survive a production build ───────────────')

// `tailwind.config.ts` scans pages/, components/ and app/ — NOT lib/. The table must
// live somewhere scanned or every chip renders colourless in the built site only.
const twConfig = readFileSync(join(WEB, 'tailwind.config.ts'), 'utf8')
const contentBlock = twConfig.slice(twConfig.indexOf('content:'), twConfig.indexOf('theme:'))
check('the product table lives in a directory Tailwind scans', /\.\/components\//.test(contentBlock))
check('...and it is in components/, not lib/', existsSync(join(WEB, 'components', 'products.ts')))
check('...and lib/ is still NOT scanned, so this matters', !/\.\/lib\//.test(contentBlock))

// Only a PRODUCTION stylesheet answers the purge question. `next dev` writes its own
// unminified CSS into the same directory, and asserting against that reports every chip
// as purged — a red line caused by having run the dev server, which is the kind of
// failure that teaches people to ignore the suite. Production files are content-hashed
// at the top level; dev nests them under `app/`.
const css = join(WEB, '.next', 'static', 'css')
const production = existsSync(css)
  ? readdirSync(css).filter(f => /^[a-f0-9]{8,}\.css$/.test(f))
  : []

if (production.length > 0) {
  const built = production.map(f => readFileSync(join(css, f), 'utf8')).join('')
  const purged = PRODUCTS.filter(p => {
    const text = p.chip.match(/(?:^|\s)text-([\w-]+)/)?.[1]
    return text && !built.includes(`text-${text}`)
  })
  check(
    `no chip palette was purged from the built stylesheet${purged.length ? ` — ${purged.map(p => p.label).join(', ')}` : ''}`,
    purged.length === 0,
  )
} else {
  console.log('SKIP  no production stylesheet — run `npm run build` to check for purged palettes')
}

console.log(`\n${failed === 0 ? 'ALL PASS' : `${failed} FAILED`}`)
process.exit(failed === 0 ? 0 : 1)
