// Pairing-aware networking.
//
// The web dashboard talks to the Rust API through a same-origin Next.js proxy
// (`app/api/[...slug]/route.ts`). A mobile (Capacitor) build has no Next server, so it
// must call the operator's *own* sniper API directly — at a base URL the user pairs to
// once, with the bearer token that unlocks the control routes (see `require_token` in
// `crates/scematica-api/src/main.rs`).
//
// `apiFetch` unifies both: with no pairing set it behaves exactly like `fetch` against
// the relative `/api/*` proxy (web is unchanged); with a pairing it targets the paired
// instance and injects the token. It is dependency-free — native detection reads the
// `window.Capacitor` global and the pairing lives in `localStorage`, which persists in
// the Capacitor WebView — so importing it never pulls a Capacitor package into the web
// bundle.

export interface Pairing {
  /** Base URL of the paired sniper's Rust API, e.g. `http://192.168.1.50:3001`. */
  baseUrl: string
  /** Bearer token matching the instance's `SCEMATICA_API_TOKEN` (control routes). */
  token?: string
  /** Optional human label for the paired instance. */
  label?: string
}

const KEY = 'scematica.pairing'

/** True inside a Capacitor native shell (Android/iOS), false in a plain browser. */
export function isNative(): boolean {
  return (
    typeof window !== 'undefined' &&
    !!(window as unknown as { Capacitor?: { isNativePlatform?: () => boolean } }).Capacitor
      ?.isNativePlatform?.()
  )
}

/**
 * True when this bundle was built as a static export (`MOBILE_EXPORT=1`), baked in at
 * build time by next.config.js. A static export ships no Next server, so the
 * `app/api/[...slug]` proxy does not exist and a relative `/api/*` call resolves against
 * whatever static host is serving the files — 404, with an HTML error page as the body.
 */
export function isStaticExport(): boolean {
  return process.env.NEXT_PUBLIC_STATIC_EXPORT === '1'
}

/**
 * True when this build has no same-origin `/api/*` proxy to fall back on, so API calls
 * have nowhere to go until the operator pairs an instance. Both the native shell and a
 * statically-exported bundle opened in a browser are in this position.
 */
export function needsPairing(): boolean {
  return (isNative() || isStaticExport()) && getPairing() === null
}

export function getPairing(): Pairing | null {
  if (typeof window === 'undefined') return null
  try {
    const raw = window.localStorage.getItem(KEY)
    return raw ? (JSON.parse(raw) as Pairing) : null
  } catch {
    return null
  }
}

export function setPairing(p: Pairing | null): void {
  if (typeof window === 'undefined') return
  try {
    if (p) window.localStorage.setItem(KEY, JSON.stringify(p))
    else window.localStorage.removeItem(KEY)
  } catch {
    /* storage disabled — pairing simply won't persist */
  }
}

/** API base for calls: the paired instance if set, else '' (same-origin web proxy). */
export function apiBase(): string {
  const p = getPairing()
  return p?.baseUrl ? p.baseUrl.replace(/\/+$/, '') : ''
}

/**
 * `fetch` targeting the paired instance (or the same-origin proxy on web), with the
 * bearer token injected when paired. `path` is always the proxy-style `/api/...` path;
 * the base is prepended only when a pairing exists.
 */
export function apiFetch(path: string, init: RequestInit = {}): Promise<Response> {
  // No proxy in this build and nothing paired: the request would hit the static host,
  // 404, and return an HTML error page that every JSON caller then fails to parse.
  // Answer locally with the same 503 the proxy uses for "no instance" instead — one
  // synthetic response beats a 404 + a parse error per panel, per poll.
  if (needsPairing()) {
    return Promise.resolve(
      new Response(
        JSON.stringify({
          error: 'no_instance_paired',
          hint: 'Pair this app with your own sniper API to load data.',
        }),
        { status: 503, headers: { 'Content-Type': 'application/json' } },
      ),
    )
  }

  const base = apiBase()
  const url = base ? base + path : path
  const p = getPairing()
  const headers = new Headers(init.headers)
  if (p?.token) headers.set('Authorization', `Bearer ${p.token}`)
  return fetch(url, { cache: 'no-store', ...init, headers })
}

/**
 * Probe a candidate pairing by hitting its `/health`. Returns true iff reachable and
 * (when a token is supplied) authorized to POST a no-op control. Used by the pairing
 * screen to validate before saving.
 */
export async function probePairing(p: Pairing): Promise<boolean> {
  const base = p.baseUrl.replace(/\/+$/, '')
  try {
    const health = await fetch(base + '/health', { cache: 'no-store' })
    if (!health.ok) return false
    // Confirm the token is accepted by reading the (gated-adjacent) controls snapshot.
    const headers = new Headers()
    if (p.token) headers.set('Authorization', `Bearer ${p.token}`)
    const ctl = await fetch(base + '/api/controls', { headers, cache: 'no-store' })
    return ctl.ok
  } catch {
    return false
  }
}
