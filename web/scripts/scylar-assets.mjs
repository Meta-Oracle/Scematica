#!/usr/bin/env node
// Converts Scylar's source expressions into web-ready WebP.
//
//   node scripts/scylar-assets.mjs        (or: npm run scylar:assets)
//
// The source art is 1254x1254 PNG at ~2.4 MB each — 7.2 MB of blocking page weight for
// three images, and the same weight again inside the Capacitor APK. WebP at q82 lands
// them near 150-250 KB with no visible loss at display size.
//
// Pre-converting rather than leaning on next/image is deliberate: the mobile build is a
// static export with `images: { unoptimized: true }` (see next.config.js), so on that
// path next/image would ship the raw PNGs into the bundle.
//
// Idempotent — safe to re-run; it overwrites its own output and touches nothing else.

import { mkdir, readdir, stat, writeFile } from 'node:fs/promises'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const SRC = resolve(HERE, '../../scylar-terminal/scylar-expressions')
const OUT = resolve(HERE, '../public/scylar')

// 512 is the display size (the portrait panel caps around 480 CSS px); 1024 covers
// high-DPI screens via srcset. Emitting 1254 would only ever be downscaled.
const WIDTHS = [512, 1024]
const QUALITY = 82

// Must match `Expression` in lib/scylar/expressions.ts. A stem here with no counterpart
// there is a sprite nothing can display.
const EXPECTED = ['idle', 'talking', 'joyous']

async function main() {
  let sharp
  try {
    sharp = (await import('sharp')).default
  } catch {
    console.error(
      'scylar-assets: `sharp` is not installed.\n' +
        '  Install it with:  npm i -D sharp\n' +
        '  (dev-only — it runs at build time, never in the browser bundle.)',
    )
    process.exit(1)
  }

  let entries
  try {
    entries = (await readdir(SRC)).filter((f) => f.toLowerCase().endsWith('.png'))
  } catch {
    console.error(`scylar-assets: no source directory at ${SRC}`)
    process.exit(1)
  }

  const stems = entries.map((f) => f.replace(/\.png$/i, ''))
  const missing = EXPECTED.filter((e) => !stems.includes(e))
  const extra = stems.filter((s) => !EXPECTED.includes(s))

  // Loud rather than silent: a missing stem means a sprite the state machine will ask
  // for and get a 404 on, and that shows up as a blank avatar mid-conversation.
  if (missing.length) {
    console.error(`scylar-assets: missing expected expression(s): ${missing.join(', ')}`)
    console.error(`  found: ${stems.join(', ') || '(none)'}`)
    process.exit(1)
  }
  if (extra.length) {
    console.warn(`scylar-assets: ignoring unrecognised file(s): ${extra.join(', ')}`)
  }

  await mkdir(OUT, { recursive: true })

  let totalIn = 0
  let totalOut = 0

  for (const stem of EXPECTED) {
    const src = join(SRC, `${stem}.png`)
    const { size: inBytes } = await stat(src)
    totalIn += inBytes

    for (const width of WIDTHS) {
      const buf = await sharp(src)
        .resize(width, width, { fit: 'cover' })
        .webp({ quality: QUALITY, effort: 6 })
        .toBuffer()

      const dest = join(OUT, `${stem}-${width}.webp`)
      await writeFile(dest, buf)
      totalOut += buf.length

      console.log(
        `  ${stem}-${width}.webp  ${(buf.length / 1024).toFixed(0)} KB` +
          `  (from ${(inBytes / 1024 / 1024).toFixed(1)} MB)`,
      )
    }
  }

  const saved = 1 - totalOut / (totalIn * WIDTHS.length)
  console.log(
    `\nscylar-assets: ${EXPECTED.length} expressions x ${WIDTHS.length} sizes -> ` +
      `${(totalOut / 1024).toFixed(0)} KB total (${(saved * 100).toFixed(0)}% smaller)`,
  )
}

main().catch((err) => {
  console.error('scylar-assets failed:', err)
  process.exit(1)
})
