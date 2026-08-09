// Scylar portrait generation — reference-conditioned image gen. **Server-only.**
//
// This is the enhancement layer, not the avatar. The three sprites in `expressions.ts`
// drive every turn; generation runs opportunistically for special moments and is
// allowed to fail, be rate-limited, or be switched off entirely without the terminal
// noticing. That inversion is deliberate and load-bearing:
//
//   - Generation takes seconds. An avatar that pauses mid-conversation reads as broken,
//     so nothing in the chat path may ever await this.
//   - Every free tier here is a *trial* (tens to low hundreds of images), not an ongoing
//     allowance. Generating per message burns it in one sitting; generating rarely and
//     caching hard makes it last.
//   - Character consistency is not free. Without reference conditioning, each generation
//     returns a different person — which is strictly worse than three consistent sprites.
//
// On consistency: IP-Adapter is the right tool for three source images. It conditions on
// a reference image with no training (~22M params), where a LoRA would want 15-30+
// images. The reference is one of the existing expressions, so generated frames inherit
// her face, palette and framing.
//
// Disabled unless SCYLAR_IMAGEGEN=1. Off is the correct default: the sprites are the
// product, and an unconfigured deploy should be fully functional, not degraded.

if (typeof window !== 'undefined') {
  throw new Error(
    'lib/scylar/portrait.ts is server-only — it reads image-provider API keys. ' +
      'Client components should call /api/scylar/portrait instead.',
  )
}

import type { Expression } from './expressions'

export interface PortraitBackend {
  id: string
  label: string
  /** Why you might pick it, and what the free allowance actually is. */
  note: string
  envVar: string
}

/**
 * Backends, in the order they are tried.
 *
 * Self-hosted first on purpose: it is the only genuinely unlimited option, and the only
 * one where "free" stays true after the first few hundred images. It costs a GPU that
 * stays up, which is a real cost — just not a per-image one.
 */
export const BACKENDS: PortraitBackend[] = [
  {
    id: 'comfyui',
    label: 'ComfyUI (self-hosted)',
    note: 'Unlimited and free in licensing; needs a GPU host running IP-Adapter.',
    envVar: 'SCYLAR_COMFYUI_URL',
  },
  {
    id: 'segmind',
    label: 'Segmind',
    note: 'Consistent-character endpoint (IP-Adapter Face + InstantID); credit-based.',
    envVar: 'SEGMIND_API_KEY',
  },
  {
    id: 'huggingface',
    label: 'HuggingFace Inference',
    note: 'Free tier, rate-limited; SD + IP-Adapter via hosted inference.',
    envVar: 'HF_API_TOKEN',
  },
]

export function imagegenEnabled(): boolean {
  return process.env.SCYLAR_IMAGEGEN === '1'
}

export function resolveBackend(): PortraitBackend | null {
  if (!imagegenEnabled()) return null
  return BACKENDS.find((b) => (process.env[b.envVar] || '').trim()) ?? null
}

/** Which reference sprite to condition on for a requested mood. */
export function referenceFor(mood: Expression): Expression {
  return mood
}

/**
 * Cache key for a generated portrait.
 *
 * Keyed on everything that changes the output, so an identical request is free on the
 * second call. With a trial-sized allowance the cache is not an optimisation — it is
 * what makes the feature affordable at all.
 */
export function cacheKey(prompt: string, reference: Expression, backend: string): string {
  // djb2 — short, stable, and sufficient for a cache key. Not a security boundary.
  const input = `${backend}::${reference}::${prompt.trim().toLowerCase()}`
  let h = 5381
  for (let i = 0; i < input.length; i++) {
    h = ((h << 5) + h + input.charCodeAt(i)) >>> 0
  }
  return `${reference}-${h.toString(36)}`
}

/** Prompt scaffold holding style fixed so only the requested change varies. */
export function buildPrompt(mood: string): string {
  return [
    'cyberpunk anime portrait of the same character,',
    'silver-violet hair in a high ponytail, violet eyes, hexagonal earrings,',
    'dark circuit-board background, deep violet and black palette,',
    `expression: ${mood},`,
    'same framing and lighting as the reference image',
  ].join(' ')
}
