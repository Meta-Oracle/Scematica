/** @type {import('next').NextConfig} */
// Web (Vercel) builds as a `standalone` Node server whose /api/* routes proxy to the
// Rust API. The mobile (Capacitor) build sets MOBILE_EXPORT=1 to emit a static `out/`
// the native shell bundles — the app then talks to the operator's paired instance
// directly via lib/net.ts (no Next server, so the proxy routes are relocated out of
// the tree during the mobile export; see scripts/mobile-export.mjs).
const mobile = process.env.MOBILE_EXPORT === '1'

const nextConfig = {
  output: mobile ? 'export' : 'standalone',
  ...(mobile ? { images: { unoptimized: true }, trailingSlash: true } : {}),
  // Baked into the client bundle so lib/net.ts knows at runtime whether a same-origin
  // /api/* proxy exists in this build. Without it, a static export polls /api/* against
  // the static host and every panel gets a 404 + an HTML body (see `needsPairing`).
  env: { NEXT_PUBLIC_STATIC_EXPORT: mobile ? '1' : '0' },
  // RUST_API_URL is server-only — never exposed to the browser.
  // Set it in Vercel → Settings → Environment Variables (no NEXT_PUBLIC_ prefix).
  // Default: http://localhost:3001 for local dev.
}

module.exports = nextConfig
