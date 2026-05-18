import { type NextRequest, NextResponse } from 'next/server'

// Server-side proxy: forwards /api/* → Rust API server.
// The Rust API URL is kept server-side only (no NEXT_PUBLIC_ prefix needed).
// Browser never touches port 3001 directly — all requests go through Next.js.
const RUST_API = process.env.RUST_API_URL || 'http://localhost:3001'

export async function GET(
  request: NextRequest,
  { params }: { params: { slug: string[] } }
) {
  const path = params.slug.join('/')
  const search = request.nextUrl.search
  const url = `${RUST_API}/api/${path}${search}`

  try {
    const res = await fetch(url, {
      headers: { 'Accept': 'application/json' },
      // No caching — live data only
      cache: 'no-store',
    })

    if (!res.ok) {
      return NextResponse.json(
        { error: `upstream ${res.status}` },
        { status: res.status }
      )
    }

    const data = await res.json()
    return NextResponse.json(data, {
      headers: { 'Cache-Control': 'no-store, max-age=0' },
    })
  } catch (err) {
    const msg = err instanceof Error ? err.message : 'upstream unreachable'
    return NextResponse.json(
      { error: msg, hint: 'Start the API server: cargo run --release --bin api' },
      { status: 503 }
    )
  }
}
