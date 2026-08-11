import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

import { NextResponse, type NextRequest } from 'next/server'

import { ComfyError, generate, probe } from '@/lib/scylar/comfyui'
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

/**
 * Where generated portraits are kept.
 *
 * The OS temp directory, deliberately not `public/`: this repo lives under OneDrive,
 * which does not read `.gitignore`, so anything written inside the tree gets synced.
 * Losing the cache on reboot costs a handful of GPU-seconds; syncing generated images to
 * cloud storage forever costs more.
 */
const CACHE_DIR = join(tmpdir(), 'scylar-portraits')

async function readCached(key: string): Promise<Buffer | null> {
  try {
    return await readFile(join(CACHE_DIR, `${key}.png`))
  } catch {
    return null
  }
}

async function writeCached(key: string, bytes: Uint8Array): Promise<void> {
  try {
    await mkdir(CACHE_DIR, { recursive: true })
    await writeFile(join(CACHE_DIR, `${key}.png`), bytes)
  } catch {
    // A cache that cannot write is slower, not broken. Never surfaced.
  }
}

export async function GET() {
  const backend = resolveBackend()

  // Probed rather than merely reported. "SCYLAR_COMFYUI_URL is set" and "ComfyUI is up
  // with the IPAdapter nodes installed" are different claims, and only the second one
  // predicts whether a POST will work — which is the question the caller is asking.
  const comfy = backend?.id === 'comfyui' ? await probe() : null

  return NextResponse.json({
    ok: Boolean(backend) && (comfy === null || (comfy.reachable && comfy.ipadapter)),
    enabled: imagegenEnabled(),
    active: backend?.id ?? null,
    ...(comfy ? { comfyui: comfy } : {}),
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

  // Only ComfyUI has a transport. Segmind and HuggingFace stay declared-but-unbuilt
  // rather than being quietly dropped from `BACKENDS`: the list is what the capability
  // probe reports, and an operator with a Segmind key deserves to be told the transport
  // is missing instead of watching resolution silently pick nothing.
  if (backend.id !== 'comfyui') {
    return NextResponse.json(
      {
        ok: false,
        error: `Backend "${backend.id}" transport is not implemented yet.`,
        detail:
          'Only the ComfyUI backend is wired. Set SCYLAR_COMFYUI_URL to use generation, ' +
          'or leave SCYLAR_IMAGEGEN unset to stay on sprites.',
        planned: { backend: backend.id, reference, prompt, cacheKey: key },
        fallback: 'Sprites continue to drive the avatar; nothing is degraded by this.',
      },
      { status: 501 },
    )
  }

  // Cache first. With generation measured in tens of seconds, a hit is the difference
  // between a portrait that can be requested on a whim and one that can't — and the
  // prompt only varies by mood, so the hit rate approaches 1 after the first few.
  const hit = await readCached(key)
  if (hit) {
    return new NextResponse(new Uint8Array(hit) as BodyInit, {
      headers: {
        'Content-Type': 'image/png',
        'Cache-Control': 'no-store',
        'X-Scylar-Portrait': 'cache',
        'X-Scylar-Portrait-Key': key,
      },
    })
  }

  let referenceBytes: Buffer
  try {
    referenceBytes = await readFile(
      join(process.cwd(), 'public', 'scylar', `${reference}-1024.webp`),
    )
  } catch {
    return NextResponse.json(
      {
        ok: false,
        error: `Reference sprite for "${reference}" is missing.`,
        detail: 'Run `npm run scylar:assets` to regenerate public/scylar/*.webp.',
      },
      { status: 500 },
    )
  }

  try {
    const image = await generate({
      referenceBytes,
      referenceName: `scylar-${reference}.webp`,
      positive: prompt,
    })
    // Written after the response is built, not awaited before it: a cache that fails to
    // write must not fail the generation the operator already paid for in GPU seconds.
    void writeCached(key, image.bytes)

    return new NextResponse(image.bytes as BodyInit, {
      headers: {
        'Content-Type': image.contentType,
        'Cache-Control': 'no-store',
        'X-Scylar-Portrait': 'generated',
        'X-Scylar-Portrait-Key': key,
      },
    })
  } catch (err) {
    if (err instanceof ComfyError) {
      return NextResponse.json(
        {
          ok: false,
          error: err.message,
          detail: err.detail,
          fallback: 'Sprites continue to drive the avatar; nothing is degraded by this.',
        },
        { status: err.status },
      )
    }
    return NextResponse.json(
      { ok: false, error: 'Portrait generation failed.', detail: String(err) },
      { status: 500 },
    )
  }
}
