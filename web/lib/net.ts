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

/**
 * Reduce a pasted base URL to the API **root** — the origin `apiFetch` may prepend to a
 * `/api/...` path.
 *
 * The trailing-`/api` strip is not cosmetic. Every caller passes a proxy-style path that
 * already starts with `/api/`, so a base of `https://host/api` produces
 * `https://host/api/api/mesh`, which 404s on every endpoint. That mistake is *easy* to
 * make and used to be *impossible to detect*, because the Rust router serves both
 * `/health` and `/api/health`: the old probe checked `<base>/health`, which the wrong
 * base satisfies via the second route. Pairing therefore reported success against a base
 * that could not serve a single data endpoint, and the only visible symptom was every
 * panel going empty — loudest on /mesh, which has no simulated fallback to hide behind.
 *
 * Nothing is lost by stripping: the API mounts its routes at the server root, so a base
 * legitimately ending in `/api` cannot exist. A reverse proxy that maps `/api/*` to the
 * API root is exactly the case this handles — the operator pairs `https://host/api` and
 * we ask for `https://host/api/mesh`, which is what their proxy expects.
 */
export function normalizeBase(raw: string): string {
  return raw.trim().replace(/\/+$/, '').replace(/\/api$/i, '')
}

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
    // Normalise on the way in as well as on the way out, so a pairing saved by an older
    // build (or edited by hand in devtools) is repaired the next time it is written.
    if (p) window.localStorage.setItem(KEY, JSON.stringify({ ...p, baseUrl: normalizeBase(p.baseUrl) }))
    else window.localStorage.removeItem(KEY)
  } catch {
    /* storage disabled — pairing simply won't persist */
  }
}

/** API base for calls: the paired instance if set, else '' (same-origin web proxy). */
export function apiBase(): string {
  const p = getPairing()
  // Normalised at read time too: a pairing written by a build that predates
  // `normalizeBase` is already in someone's localStorage, and it must start working
  // without them noticing they need to re-pair.
  return p?.baseUrl ? normalizeBase(p.baseUrl) : ''
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
  /** Something answered, but it is not a scematica-api root — the paths do not exist. */
  | { ok: false; reason: 'not-an-api' }
  /** An `http://` instance read from an `https://` page: blocked before it is sent. */
  | { ok: false; reason: 'mixed-content' }

/**
 * True when this page is HTTPS and `base` is HTTP. The browser refuses the request as
 * mixed content *before* it leaves, so there is no status code and `fetch` rejects with
 * a bare `TypeError` — indistinguishable from a firewall unless it is checked up front.
 * This is the normal state of affairs on a Vercel deploy pointed at a LAN instance.
 */
export function isMixedContent(base: string): boolean {
  if (typeof window === 'undefined') return false
  return window.location.protocol === 'https:' && normalizeBase(base).startsWith('http://')
}

/**
 * Validate a candidate pairing before saving it: reachable, rooted where `apiFetch`
 * expects, and carrying a token the instance actually accepts.
 *
 * **The reachability check must use a path under `/api/`, not `/health`.** The Rust
 * router serves both `/health` and `/api/health`, so a base of `https://host/api` — the
 * single most common way to get this wrong — satisfies `<base>/health` and the old probe
 * declared success. Every subsequent call then asked for `/api/api/*` and 404'd, with no
 * error anywhere pointing at the URL. Probing `<base>/api/health` has no such alias: it
 * resolves only when `base` is the true root. `normalizeBase` now repairs that input
 * anyway, so this is the backstop for the *other* wrong roots (a site root, a tunnel
 * landing page, a proxy that swallows unknown paths).
 *
 * Getting the token half right is separately fiddly. `GET /api/controls` is NOT
 * token-gated — only the control **POSTs** carry `require_token` (see `main.rs`, where
 * the gated router holds `post(...)` routes and the plain router serves `get(controls)`),
 * so probing the GET reports success for any token at all, including none.
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
  const base = normalizeBase(p.baseUrl)

  // Checked before the request, because the browser refuses it silently and the
  // resulting TypeError is otherwise reported as a firewall problem.
  if (isMixedContent(base)) return { ok: false, reason: 'mixed-content' }

  try {
    const health = await fetch(base + '/api/health', { cache: 'no-store' })
    // A 404 here means something is listening but it is not a scematica-api root —
    // a different claim from "nothing answered", and it needs a different fix.
    if (health.status === 404) return { ok: false, reason: 'not-an-api' }
    if (!health.ok) return { ok: false, reason: 'unreachable' }
    // A tunnel landing page or an SPA catch-all answers 200 with HTML. Requiring JSON
    // is what separates "the API is here" from "something is here".
    await health.json()
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
    if (res.status === 404) return { ok: false, reason: 'not-an-api' }
    return { ok: true }
  } catch {
    return { ok: false, reason: 'unreachable' }
  }
}
