import { type NextRequest, NextResponse } from 'next/server'

const RUST_API = process.env.RUST_API_URL || 'http://localhost:3001'

async function proxy(request: NextRequest, params: { slug: string[] }) {
  const path = params.slug.join('/')
  const search = request.nextUrl.search
  const url = `${RUST_API}/api/${path}${search}`

  try {
    const isPost = request.method === 'POST'
    const body = isPost ? await request.text() : undefined

    const res = await fetch(url, {
      method: request.method,
      headers: {
        'Accept': 'application/json',
        ...(isPost ? { 'Content-Type': 'application/json' } : {}),
      },
      body,
      cache: 'no-store',
      signal: AbortSignal.timeout(5_000),
    })

    if (!res.ok) {
      return NextResponse.json({ error: `upstream ${res.status}` }, { status: res.status })
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

export async function GET(req: NextRequest, { params }: { params: { slug: string[] } }) {
  return proxy(req, params)
}

export async function POST(req: NextRequest, { params }: { params: { slug: string[] } }) {
  return proxy(req, params)
}
