/**
 * Service worker: the only place the pairing token exists.
 *
 * ## Why the token never reaches the content script
 *
 * A content script runs in an isolated world, but it runs *on the page* — same process,
 * same tab, and one `executeScript` bug or one prototype-pollution trick away from the
 * page's own code. If it held the daemon token, a hostile page that got at it could drive
 * the operator's agent over `127.0.0.1` for as long as the pairing lasts.
 *
 * So the split is:
 *
 *   content script  →  "here is a world, please simulate it"   (no credentials)
 *   service worker  →  adds the Authorization header, fetches  (holds the secret)
 *   service worker  →  returns the rendered result
 *
 * The content script cannot name a URL either. It sends a message *type*, and this file
 * maps the type to a path — the same rule as `lib/scylar/tools.ts` in the web app, where
 * the model picks a tool name and never an endpoint. A page that manages to post a crafted
 * message still cannot make the worker talk to anything but the daemon.
 *
 * ## Parameterised routes are the one crack in that, and it is nailed shut
 *
 * `GET /decisions/{id}` needs a caller-supplied segment, which is exactly the shape of
 * request that turns "pick a tool" back into "pick a URL". [`ROUTES.record`] therefore
 * declares a `param` with a regular expression, the id is matched against it before the
 * path is built, and it is `encodeURIComponent`d afterwards. A record id is eight hex
 * characters; anything that is not is refused here rather than being sent and 404ing, so a
 * `../` never reaches the daemon's router in the first place.
 *
 * ## Why this fetches instead of the content script
 *
 * A `fetch` from a content script is subject to CORS, and the daemon deliberately sends no
 * `Access-Control-Allow-Origin`. A fetch from the service worker under `host_permissions`
 * is not. That is the whole reason the daemon can refuse CORS outright and still be
 * usable — a web page gets no way to read a reply, and this extension does.
 */

const DEFAULT_BASE = 'http://127.0.0.1:7842';

/**
 * Message type → daemon route. A caller may pick a key here and nothing else.
 *
 * `decide` is present and it is *not* a mistake: the daemon refuses it with 403 unless it
 * was started with `--allow-decide`, so the authority lives where it can be audited rather
 * than in whether this table happens to list the route. A client that cannot even name the
 * endpoint would make "why did nothing happen" harder to answer, not safer.
 */
const ROUTES = {
  health: { method: 'GET', path: '/health', auth: false },
  policy: { method: 'GET', path: '/policy', auth: true },
  observe: { method: 'POST', path: '/observe', auth: true },
  simulate: { method: 'POST', path: '/simulate', auth: true },
  decide: { method: 'POST', path: '/decide', auth: true },
  decisions: { method: 'GET', path: '/decisions', auth: true },
  memory: { method: 'GET', path: '/memory/stats', auth: true },
  record: {
    method: 'GET',
    // Built from `param`, never from a caller-supplied string.
    path: (id) => `/decisions/${encodeURIComponent(id)}`,
    auth: true,
    param: { name: 'id', pattern: /^[0-9a-fA-F]{4,64}$/ },
  },
};

/** Strip a trailing slash, and a pasted endpoint path, so `base + path` never doubles. */
function normalizeBase(raw) {
  let s = (raw || '').trim().replace(/\/+$/, '');
  // Somebody will paste the URL they just curled. Stripped on read as well as on write, so
  // an old pairing in storage starts working without a re-pair — the same fix
  // `web/lib/net.ts::normalizeBase` needed for the `/mesh` panel.
  s = s.replace(/\/(health|policy|observe|simulate|decide|decisions|memory)$/i, '');
  return s || DEFAULT_BASE;
}

async function pairing() {
  const stored = await chrome.storage.local.get(['baseUrl', 'token']);
  return { base: normalizeBase(stored.baseUrl), token: stored.token || '' };
}

/**
 * Call the daemon.
 *
 * Errors are returned as `{ ok: false, reason, detail }` rather than thrown, and the
 * reasons are distinguished on purpose: "not paired", "daemon unreachable" and "token
 * rejected" send an operator to three different places, and collapsing them into "failed"
 * is the mistake `/mesh` made when every failure rendered as "No instance paired".
 */
async function call(type, body) {
  const route = ROUTES[type];
  if (!route) return { ok: false, reason: 'unknown_request', detail: String(type) };

  let path = route.path;
  if (route.param) {
    const value = body && body[route.param.name];
    if (typeof value !== 'string' || !route.param.pattern.test(value)) {
      return {
        ok: false,
        reason: 'bad_parameter',
        detail: `\`${route.param.name}\` must match ${route.param.pattern}`,
      };
    }
    path = route.path(value);
    body = undefined;
  }

  const { base, token } = await pairing();
  if (route.auth && !token) {
    return {
      ok: false,
      reason: 'not_paired',
      detail: 'No token. Open the extension options and paste the one from .scema/omnid.token.',
    };
  }

  const headers = { 'Content-Type': 'application/json' };
  if (route.auth) headers.Authorization = `Bearer ${token}`;

  let res;
  try {
    res = await fetch(base + path, {
      method: route.method,
      headers,
      body: route.method === 'POST' ? JSON.stringify(body || {}) : undefined,
    });
  } catch (e) {
    return {
      ok: false,
      reason: 'unreachable',
      detail: `Could not reach ${base}${path}. Is scema-omnid running? (${e.message})`,
    };
  }

  const text = await res.text();
  let parsed = null;
  try {
    parsed = JSON.parse(text);
  } catch {
    // A tunnel or proxy login page answers 200 with HTML. Treating that as a daemon reply
    // is how a wrong base URL looks like a broken daemon.
    return {
      ok: false,
      reason: 'malformed',
      detail: `${base} answered ${res.status} with something that is not JSON.`,
    };
  }

  if (res.status === 401) {
    return {
      ok: false,
      reason: 'rejected',
      detail: 'The daemon rejected this token. Re-copy it from .scema/omnid.token.',
    };
  }
  if (res.status === 421) {
    return {
      ok: false,
      reason: 'bad_host',
      detail:
        'The daemon answers only to 127.0.0.1 or localhost on its own port. A base URL naming anything else is refused before it is read.',
    };
  }
  if (!res.ok) {
    return {
      ok: false,
      reason: 'error',
      detail: parsed.message || `HTTP ${res.status}`,
      status: res.status,
      code: parsed.error,
    };
  }
  return { ok: true, data: parsed, base };
}

// Guarded so this file also loads under `node --test`, where there is no `chrome`. The
// route table and the base-URL normaliser are pure and are worth testing without a browser;
// registering listeners at import time would make that impossible.
const inExtension = typeof chrome !== 'undefined' && !!(chrome.runtime && chrome.runtime.onMessage);

if (inExtension) {
  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    // `msg.type` is looked up in ROUTES; `msg.body` is forwarded. Nothing from the page can
    // choose a host, a path or a method.
    call(msg && msg.type, msg && msg.body).then(sendResponse);
    return true; // async response
  });
}

/**
 * Perception is opt-in, per invocation.
 *
 * There is no `content_scripts` block in the manifest and no `<all_urls>` host permission.
 * Nothing is read until the operator asks — through the popup's button, or through the
 * keyboard shortcut below — at which point `activeTab` grants access to that one tab for
 * that one interaction. An agent runtime that quietly read every page you visited would be
 * the wrong shape of thing to install, whatever it did with the data.
 *
 * The keyboard path exists because the popup added a click. `activeTab` is granted by a
 * command invocation exactly as it is by a toolbar click, so this is the same permission
 * with one fewer step, not a way around one.
 */
async function inject(tab) {
  if (!tab || !tab.id) return;
  try {
    await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      files: ['src/theme.js', 'src/perceive.js', 'src/content.js'],
    });
  } catch (e) {
    // Chrome refuses injection into its own pages and the Web Store. Nothing to do about
    // it here — the popup checks the URL up front and says so, which is where an operator
    // will actually see it.
    console.warn('[scema] cannot inject here:', e.message);
  }
}

if (inExtension && chrome.commands) {
  chrome.commands.onCommand.addListener(async (command) => {
    if (command !== 'perceive-page') return;
    const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
    await inject(tab);
  });
}

/**
 * Toolbar click, for the case where there is no popup.
 *
 * In MV3 `action.onClicked` does **not** fire when `action.default_popup` is set, so on a
 * current manifest this listener is dead code. It is here for the one case that is
 * otherwise unrecoverable and looks exactly like a broken extension: a browser still
 * holding a **stale manifest** — loaded unpacked before `popup.html` existed, and not
 * reloaded since. That manifest has no popup, so Chrome dispatches the click here; without
 * a listener the icon does nothing at all, with no error anywhere the operator would look.
 *
 * Reload the extension and this stops being reachable. Keeping it costs nothing and turns a
 * silent dead button into the old, working behaviour.
 */
if (inExtension && chrome.action && chrome.action.onClicked) {
  chrome.action.onClicked.addListener(async (tab) => {
    console.warn(
      '[scema] toolbar click reached the service worker, which means this browser is ' +
        'running a manifest with no popup. Reload the extension on chrome://extensions ' +
        'to pick up the current one.'
    );
    await inject(tab);
  });
}

// Exported for `test/routes.test.js`, which runs this file under Node with no `chrome`.
if (typeof module !== 'undefined' && module.exports) {
  module.exports = { ROUTES, normalizeBase, DEFAULT_BASE };
}
