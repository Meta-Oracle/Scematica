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
 * maps the type to a path on the paired base — the same rule as `lib/scylar/tools.ts` in
 * the web app, where the model picks a tool name and never an endpoint. A page that
 * manages to post a crafted message still cannot make the worker talk to anything but the
 * daemon.
 *
 * ## Why this fetches instead of the content script
 *
 * A `fetch` from a content script is subject to CORS, and the daemon deliberately sends no
 * `Access-Control-Allow-Origin`. A fetch from the service worker under `host_permissions`
 * is not. That is the whole reason the daemon can refuse CORS outright and still be
 * usable — a web page gets no way to read a reply, and this extension does.
 */

const DEFAULT_BASE = 'http://127.0.0.1:7842';

/** Message type → daemon path. The content script may pick a key here and nothing else. */
const ROUTES = {
  health: { method: 'GET', path: '/health', auth: false },
  policy: { method: 'GET', path: '/policy', auth: true },
  simulate: { method: 'POST', path: '/simulate', auth: true },
};

/** Strip a trailing slash so `base + path` never doubles it. */
function normalizeBase(raw) {
  const s = (raw || '').trim().replace(/\/+$/, '');
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
  if (!route) return { ok: false, reason: 'unknown_request', detail: type };

  const { base, token } = await pairing();
  if (route.auth && !token) {
    return { ok: false, reason: 'not_paired', detail: 'No token. Open the extension options and paste the one from .scema/omnid.token.' };
  }

  const headers = { 'Content-Type': 'application/json' };
  if (route.auth) headers.Authorization = `Bearer ${token}`;

  let res;
  try {
    res = await fetch(base + route.path, {
      method: route.method,
      headers,
      body: route.method === 'POST' ? JSON.stringify(body || {}) : undefined,
    });
  } catch (e) {
    return {
      ok: false,
      reason: 'unreachable',
      detail: `Could not reach ${base}${route.path}. Is scema-omnid running? (${e.message})`,
    };
  }

  const text = await res.text();
  let parsed = null;
  try {
    parsed = JSON.parse(text);
  } catch {
    // A tunnel or proxy login page answers 200 with HTML. Treating that as a daemon reply
    // is how a wrong base URL looks like a broken daemon.
    return { ok: false, reason: 'malformed', detail: `${base} answered ${res.status} with something that is not JSON.` };
  }

  if (res.status === 401) {
    return { ok: false, reason: 'rejected', detail: 'The daemon rejected this token. Re-copy it from .scema/omnid.token.' };
  }
  if (!res.ok) {
    return { ok: false, reason: 'error', detail: parsed.message || `HTTP ${res.status}`, status: res.status, code: parsed.error };
  }
  return { ok: true, data: parsed, base };
}

chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
  // `msg.type` is looked up in ROUTES; `msg.body` is forwarded. Nothing from the page can
  // choose a host, a path or a method.
  call(msg && msg.type, msg && msg.body).then(sendResponse);
  return true; // async response
});

/**
 * Perception is opt-in, per click.
 *
 * There is no `content_scripts` block in the manifest and no `<all_urls>` host permission.
 * Nothing is read until the operator clicks the toolbar button, at which point `activeTab`
 * grants access to that one tab for that one interaction. An agent runtime that quietly
 * read every page you visited would be the wrong shape of thing to install, whatever it did
 * with the data.
 */
chrome.action.onClicked.addListener(async (tab) => {
  if (!tab || !tab.id) return;
  try {
    await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      files: ['src/perceive.js', 'src/content.js'],
    });
  } catch (e) {
    // Chrome refuses injection into its own pages and the Web Store. Nothing to do about
    // it, and a silent failure would look like a broken extension.
    console.warn('[scema] cannot inject here:', e.message);
  }
});
