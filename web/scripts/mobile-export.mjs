#!/usr/bin/env node
// Build the static export for the Capacitor mobile shell.
//
// `output: 'export'` cannot include dynamic Next route handlers, and the mobile app
// doesn't need them (it calls the paired Rust API directly via lib/net.ts). So we
// temporarily relocate the server-only `app/api` proxy tree, run the export, then
// always restore it — even if the build fails.
import { execSync } from 'node:child_process'
import { existsSync, renameSync, rmSync } from 'node:fs'
import { join } from 'node:path'

const root = process.cwd()
const apiDir = join(root, 'app', 'api')
const stash = join(root, 'app', '_api_disabled_for_export')

function restore() {
  if (existsSync(stash)) {
    if (existsSync(apiDir)) rmSync(apiDir, { recursive: true, force: true })
    renameSync(stash, apiDir)
  }
}

process.on('exit', restore)
process.on('SIGINT', () => { restore(); process.exit(1) })

try {
  if (existsSync(apiDir)) renameSync(apiDir, stash)
  console.log('[mobile-export] building static export (MOBILE_EXPORT=1)…')
  execSync('next build', { stdio: 'inherit', env: { ...process.env, MOBILE_EXPORT: '1' } })
  console.log('[mobile-export] done → out/')
} finally {
  restore()
}
