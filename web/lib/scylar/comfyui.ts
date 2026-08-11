// ComfyUI transport for Scylar's portrait generation. **Server-only.**
//
// The one backend where "free" stays true past the first few hundred images, and the only
// one where character consistency is fully under our control rather than a vendor's
// preset. It costs a GPU host that stays up, which is a real cost — just not a per-image
// one.
//
// Three things about this file are load-bearing:
//
//   1. **Nothing in the chat path awaits it.** Generation takes seconds to tens of
//      seconds; an avatar that freezes mid-conversation reads as broken. The sprites in
//      `expressions.ts` drive every turn and this decorates.
//   2. **The reference image is the product.** Without IP-Adapter conditioning on one of
//      her three sprites, every generation returns a different person — strictly worse
//      than three consistent frames. The prompt only ever varies the expression; face,
//      palette and framing come from the reference.
//   3. **No fabrication, same as everywhere else.** Unreachable host, missing node,
//      missing model and timeout each return a distinct error naming the fix. A sprite
//      handed back labelled as a generation would make a broken install look like a
//      working one, which is the same rule `/api/alchem/*` follows for prices.
//
// Guarded like `provider.ts` and `lib/alchem/endpoint.ts`: the host URL is server
// configuration, and importing this in a browser should fail loudly rather than silently
// resolve to "not configured".

if (typeof window !== 'undefined') {
  throw new Error(
    'lib/scylar/comfyui.ts is server-only — it reads the ComfyUI host URL. ' +
      'Client components should call /api/scylar/portrait instead.',
  )
}

import { randomUUID } from 'node:crypto'

/** How long a single generation may run before we give up on it. */
const GENERATION_TIMEOUT_MS = 120_000

/** Gap between `/history` polls. ComfyUI has no completion callback over HTTP. */
const POLL_INTERVAL_MS = 750

/** Short — this only ever talks to a host on the LAN or localhost. */
const HTTP_TIMEOUT_MS = 15_000

/**
 * Checkpoint to sample from.
 *
 * Overridable because the right answer depends on what the operator has downloaded, and a
 * hardcoded filename produces ComfyUI's least helpful error (`value not in list`) on any
 * install that chose differently.
 */
const CHECKPOINT = process.env.SCYLAR_COMFYUI_CHECKPOINT || 'v1-5-pruned-emaonly.safetensors'

/**
 * IPAdapter preset, passed to `IPAdapterUnifiedLoader`.
 *
 * The unified loader resolves the IP-Adapter weights *and* the matching CLIP vision
 * encoder from this one string. Naming both models explicitly instead would mean two more
 * filenames to keep in sync with whatever is on disk, for no gain.
 */
const IPADAPTER_PRESET = process.env.SCYLAR_COMFYUI_PRESET || 'PLUS (high strength)'

/**
 * How strongly the reference image dominates.
 *
 * High on purpose. The whole reason for this backend is that her face stays hers; a
 * weight low enough to let the prompt restyle her defeats the point of conditioning at
 * all. Tuned down only if generations stop responding to the mood term.
 */
const IPADAPTER_WEIGHT = Number(process.env.SCYLAR_COMFYUI_WEIGHT || '0.85')

/** Kept out of the shared prompt builder: it never varies with mood. */
const NEGATIVE_PROMPT =
  'photorealistic, 3d render, watermark, signature, text, extra fingers, deformed hands, ' +
  'blurry, lowres, jpeg artifacts, multiple characters, different face, off-model'

export interface ComfyImage {
  bytes: Uint8Array
  contentType: string
}

export class ComfyError extends Error {
  constructor(
    message: string,
    /** HTTP status the route should surface. */
    readonly status: number,
    /** What the operator should actually do about it. */
    readonly detail?: string,
  ) {
    super(message)
    this.name = 'ComfyError'
  }
}

/** Configured host, without a trailing slash, or `null`. */
export function comfyUrl(): string | null {
  const raw = (process.env.SCYLAR_COMFYUI_URL || '').trim()
  return raw ? raw.replace(/\/$/, '') : null
}

async function comfyFetch(path: string, init?: RequestInit): Promise<Response> {
  const base = comfyUrl()
  if (!base) {
    throw new ComfyError('SCYLAR_COMFYUI_URL is not set.', 503)
  }
  try {
    return await fetch(`${base}${path}`, {
      ...init,
      cache: 'no-store',
      signal: AbortSignal.timeout(HTTP_TIMEOUT_MS),
    })
  } catch (err) {
    const aborted = err instanceof Error && err.name === 'TimeoutError'
    throw new ComfyError(
      aborted ? `ComfyUI did not respond within ${HTTP_TIMEOUT_MS / 1000}s.` : 'Could not reach ComfyUI.',
      502,
      `Tried ${base}${path}. Is ComfyUI running? Start it with \`comfy launch\`.`,
    )
  }
}

/** Is the host up, and does it have the IPAdapter nodes installed? */
export async function probe(): Promise<{ reachable: boolean; ipadapter: boolean; detail?: string }> {
  let res: Response
  try {
    res = await comfyFetch('/object_info')
  } catch (err) {
    return {
      reachable: false,
      ipadapter: false,
      detail: err instanceof ComfyError ? err.detail || err.message : String(err),
    }
  }
  if (!res.ok) return { reachable: false, ipadapter: false, detail: `/object_info returned ${res.status}` }

  const info = (await res.json().catch(() => ({}))) as Record<string, unknown>
  // Checked by name rather than by attempting a generation: a missing custom node fails
  // deep inside the queue with an error the HTTP caller never sees, so the useful check
  // is the one that happens before anything is queued.
  const ipadapter = 'IPAdapterUnifiedLoader' in info && 'IPAdapterAdvanced' in info
  return {
    reachable: true,
    ipadapter,
    detail: ipadapter
      ? undefined
      : 'ComfyUI is up but the IPAdapter nodes are missing. Install them with ' +
        '`comfy node install comfyui_ipadapter_plus`, then restart ComfyUI.',
  }
}

/**
 * Put the reference sprite where ComfyUI's `LoadImage` can see it.
 *
 * Uploaded every generation rather than copied into the install's `input/` once. The
 * upload is a few hundred KB against localhost, and the alternative couples a web deploy
 * to the filesystem layout of a host that may not even be the same machine.
 */
async function uploadReference(bytes: Uint8Array, filename: string): Promise<string> {
  const form = new FormData()
  form.append('image', new Blob([bytes as BlobPart], { type: 'image/webp' }), filename)
  // `overwrite` keeps the input directory from filling with `scylar-idle (17).webp`.
  form.append('overwrite', 'true')

  const res = await comfyFetch('/upload/image', { method: 'POST', body: form })
  if (!res.ok) {
    throw new ComfyError(`ComfyUI rejected the reference upload (${res.status}).`, 502)
  }
  const body = (await res.json().catch(() => ({}))) as { name?: string; subfolder?: string }
  if (!body.name) throw new ComfyError('ComfyUI returned no filename for the upload.', 502)
  return body.subfolder ? `${body.subfolder}/${body.name}` : body.name
}

/**
 * The generation graph, in ComfyUI's API format.
 *
 * Node ids are strings and links are `[nodeId, outputIndex]` — this is the format the
 * `/prompt` endpoint takes, which is *not* the format the ComfyUI web editor saves by
 * default (that one is "workflow" format and includes layout). Exporting from the editor
 * requires "Save (API Format)"; getting this wrong yields a 400 with no useful body.
 */
export function buildWorkflow(opts: {
  reference: string
  positive: string
  seed: number
  steps?: number
  cfg?: number
  size?: number
}): Record<string, unknown> {
  const { reference, positive, seed, steps = 24, cfg = 6.5, size = 512 } = opts

  return {
    '1': { class_type: 'CheckpointLoaderSimple', inputs: { ckpt_name: CHECKPOINT } },
    '2': { class_type: 'LoadImage', inputs: { image: reference, upload: 'image' } },
    '3': {
      class_type: 'IPAdapterUnifiedLoader',
      inputs: { model: ['1', 0], preset: IPADAPTER_PRESET },
    },
    '4': {
      class_type: 'IPAdapterAdvanced',
      inputs: {
        model: ['3', 0],
        ipadapter: ['3', 1],
        image: ['2', 0],
        weight: IPADAPTER_WEIGHT,
        // Style *and* composition: framing is part of what makes the three sprites read
        // as the same character, so letting the sampler re-frame her undoes half the
        // consistency the reference is there to provide.
        weight_type: 'style and composition',
        combine_embeds: 'concat',
        start_at: 0.0,
        end_at: 1.0,
        embeds_scaling: 'V only',
      },
    },
    '5': { class_type: 'CLIPTextEncode', inputs: { clip: ['1', 1], text: positive } },
    '6': { class_type: 'CLIPTextEncode', inputs: { clip: ['1', 1], text: NEGATIVE_PROMPT } },
    '7': { class_type: 'EmptyLatentImage', inputs: { width: size, height: size, batch_size: 1 } },
    '8': {
      class_type: 'KSampler',
      inputs: {
        model: ['4', 0],
        positive: ['5', 0],
        negative: ['6', 0],
        latent_image: ['7', 0],
        seed,
        steps,
        cfg,
        sampler_name: 'dpmpp_2m',
        scheduler: 'karras',
        denoise: 1.0,
      },
    },
    '9': { class_type: 'VAEDecode', inputs: { samples: ['8', 0], vae: ['1', 2] } },
    '10': { class_type: 'SaveImage', inputs: { images: ['9', 0], filename_prefix: 'scylar' } },
  }
}

interface HistoryEntry {
  status?: { completed?: boolean; status_str?: string; messages?: unknown[] }
  outputs?: Record<string, { images?: { filename: string; subfolder: string; type: string }[] }>
}

/**
 * Queue a graph and wait for its image.
 *
 * Polls `/history` rather than opening the WebSocket. The socket carries progress events
 * this has no use for, and a route handler that holds a WebSocket open is a worse fit for
 * a serverless deploy than one that makes short polling requests.
 */
export async function generate(opts: {
  referenceBytes: Uint8Array
  referenceName: string
  positive: string
  seed?: number
}): Promise<ComfyImage> {
  const reference = await uploadReference(opts.referenceBytes, opts.referenceName)
  const seed = opts.seed ?? Math.floor(Math.random() * 2 ** 31)
  const workflow = buildWorkflow({ reference, positive: opts.positive, seed })

  const queued = await comfyFetch('/prompt', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ prompt: workflow, client_id: randomUUID() }),
  })

  if (!queued.ok) {
    // ComfyUI puts the *actual* reason here — a missing checkpoint filename, an unknown
    // node type — and it is the only place it appears. Passing it through is the
    // difference between a debuggable failure and "500".
    const detail = await queued.text().catch(() => '')
    throw new ComfyError(
      `ComfyUI rejected the workflow (${queued.status}).`,
      502,
      detail.slice(0, 600) ||
        `Check that "${CHECKPOINT}" exists in models/checkpoints and the IPAdapter nodes are installed.`,
    )
  }

  const { prompt_id: promptId } = (await queued.json().catch(() => ({}))) as { prompt_id?: string }
  if (!promptId) throw new ComfyError('ComfyUI accepted the workflow but returned no prompt_id.', 502)

  const deadline = Date.now() + GENERATION_TIMEOUT_MS
  for (;;) {
    if (Date.now() > deadline) {
      throw new ComfyError(
        `Generation did not finish within ${GENERATION_TIMEOUT_MS / 1000}s.`,
        504,
        'The job may still be queued in ComfyUI. Check its console for a stuck or ' +
          'out-of-memory run.',
      )
    }
    await new Promise((r) => setTimeout(r, POLL_INTERVAL_MS))

    const res = await comfyFetch(`/history/${promptId}`)
    if (!res.ok) continue

    const history = (await res.json().catch(() => ({}))) as Record<string, HistoryEntry>
    const entry = history[promptId]
    if (!entry) continue

    // A failed job is *present* in history with no outputs, so "no outputs yet" and
    // "finished and produced nothing" look identical unless the status is read.
    const statusStr = entry.status?.status_str
    if (statusStr === 'error') {
      throw new ComfyError(
        'ComfyUI reported an error running the workflow.',
        502,
        'See the ComfyUI console. On an 8 GB card the usual cause is an out-of-memory ' +
          'run — lower the size or use a smaller checkpoint.',
      )
    }

    const image = Object.values(entry.outputs ?? {}).flatMap((o) => o.images ?? [])[0]
    if (!image) continue

    const view = await comfyFetch(
      `/view?filename=${encodeURIComponent(image.filename)}` +
        `&subfolder=${encodeURIComponent(image.subfolder)}` +
        `&type=${encodeURIComponent(image.type)}`,
    )
    if (!view.ok) throw new ComfyError(`Could not fetch the generated image (${view.status}).`, 502)

    return {
      bytes: new Uint8Array(await view.arrayBuffer()),
      contentType: view.headers.get('Content-Type') || 'image/png',
    }
  }
}
