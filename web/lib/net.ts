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

export type ProbeResult =
  | { ok: true }
  /** Nothing answered at the base URL — wrong host/port, or a firewall in the way. */
  | { ok: false; reason: 'unreachable' }
  /** The instance answered but rejected the token. */
  | { ok: false; reason: 'unauthorized' }

/**
 * Validate a candidate pairing before saving it: reachable, and carrying a token the
 * instance actually accepts.
 *
 * Getting the second half right is fiddly. `GET /api/controls` is NOT token-gated —
 * only the control **POSTs** carry `require_token` (see `main.rs`, where the gated
 * router holds `post(...)` routes and the plain router serves `get(controls)`), so
 * probing the GET reports success for any token at all, including none.
 *
 * So the probe POSTs to a gated route instead, with a deliberately malformed body.
 * Axum runs the `route_layer` auth middleware *before* the `Json` extractor, giving
 * three clean outcomes:
 *
 *   • bad/missing token  → 401 from the middleware; the handler never runs
 *   • good token         → 4xx from the extractor rejecting the body; still no handler
 *   • no token required  → same as above
 *
 * The malformed body is the point: `params_handler` writes `scematica-rate-mode.json`,
 * so a *valid* probe body would silently rewrite the operator's live TP/SL. A JSON
 * string cannot deserialize into any of the control structs, so the handler is
 * unreachable by construction.
 */
export async function probePairing(p: Pairing): Promise<ProbeResult> {
  const base = p.baseUrl.replace(/\/+$/, '')

  try {
    const health = await fetch(base + '/health', { cache: 'no-store' })
    if (!health.ok) return { ok: false, reason: 'unreachable' }
  } catch {
    return { ok: false, reason: 'unreachable' }
  }

  const headers = new Headers({ 'Content-Type': 'application/json' })
  if (p.token) headers.set('Authorization', `Bearer ${p.token}`)

  try {
    const res = await fetch(base + '/api/controls/params', {
      method: 'POST',
      headers,
      body: '"scematica-pairing-probe"',
      cache: 'no-store',
    })
    if (res.status === 401 || res.status === 403) return { ok: false, reason: 'unauthorized' }
    return { ok: true }
  } catch {
    return { ok: false, reason: 'unreachable' }
  }
}
