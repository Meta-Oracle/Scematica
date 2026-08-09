import { NextResponse, type NextRequest } from 'next/server'

import {
  BACKENDS,
  buildPrompt,
  cacheKey,
  imagegenEnabled,
  referenceFor,
  resolveBackend,
} from '@/lib/scylar/portrait'
import { EXPRESSIONS, type Expression } from '@/lib/scylar/expressions'

// Reference-conditioned portrait generation.
//
//   GET  /api/scylar/portrait            -> capability probe
//   POST /api/scylar/portrait  { mood }  -> generated portrait, or an honest refusal
//
// Off unless SCYLAR_IMAGEGEN=1 and a backend key is present. When off this returns 501
// with the reason — it does not fall back to returning a sprite dressed up as generated
// output. The caller already has the sprites; handing one back labelled as a generation
// would make a disabled feature look like a working one.
//
// Nothing in the chat path awaits this. See lib/scylar/portrait.ts for why.

export const dynamic = 'force-dynamic'
export const runtime = 'nodejs'

function isExpression(v: unknown): v is Expression {
  return typeof v === 'string' && (EXPRESSIONS as readonly string[]).includes(v)
}

export async function GET() {
  const backend = resolveBackend()
  return NextResponse.json({
    ok: Boolean(backend),
    enabled: imagegenEnabled(),
    active: backend?.id ?? null,
    backends: BACKENDS.map(({ id, label, note, envVar }) => ({
      id,
      label,
      note,
      configured: Boolean((process.env[envVar] || '').trim()),
    })),
  })
}

export async function POST(request: NextRequest) {
  if (!imagegenEnabled()) {
    return NextResponse.json(
      {
        ok: false,
        error: 'Portrait generation is disabled.',
        detail: 'Set SCYLAR_IMAGEGEN=1 and configure a backend to enable it.',
        // Said plainly so the caller keeps using sprites rather than retrying.
        fallback: 'The three sprite expressions remain fully functional without this.',
      },
      { status: 501 },
    )
  }

  const backend = resolveBackend()
  if (!backend) {
    return NextResponse.json(
      {
        ok: false,
        error: 'Portrait generation is enabled but no backend is configured.',
        detail: `Set one of ${BACKENDS.map((b) => b.envVar).join(', ')}.`,
        backends: BACKENDS.map((b) => ({ id: b.id, note: b.note })),
      },
      { status: 503 },
    )
  }

  let body: unknown
  try {
    body = await request.json()
  } catch {
    return NextResponse.json({ ok: false, error: 'Malformed JSON body.' }, { status: 400 })
  }

  const mood = (body as { mood?: unknown })?.mood
  if (!isExpression(mood)) {
    return NextResponse.json(
      { ok: false, error: `mood must be one of: ${EXPRESSIONS.join(', ')}` },
      { status: 400 },
    )
  }

  const reference = referenceFor(mood)
  const prompt = buildPrompt(mood)
  const key = cacheKey(prompt, reference, backend.id)

  // The backend call itself is intentionally unimplemented. Each of the three has a
  // different request shape and a different consistency mechanism, and wiring one
  // blind — against an API whose free allowance is a trial — would produce code nobody
  // has watched succeed. The contract, the gating, the cache key and the refusal path
  // are real; the transport is the part to fill in once a backend is chosen.
  return NextResponse.json(
    {
      ok: false,
      error: `Backend "${backend.id}" transport is not implemented yet.`,
      detail:
        'Resolution, gating and caching are wired; the provider request is the remaining step.',
      planned: { backend: backend.id, reference, prompt, cacheKey: key },
      fallback: 'Sprites continue to drive the avatar; nothing is degraded by this.',
    },
    { status: 501 },
  )
}
