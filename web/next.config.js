/** @type {import('next').NextConfig} */
const nextConfig = {
  output: 'standalone',
  // RUST_API_URL is server-only — never exposed to the browser.
  // Set it in Vercel → Settings → Environment Variables (no NEXT_PUBLIC_ prefix).
  // Default: http://localhost:3001 for local dev.
}

module.exports = nextConfig
