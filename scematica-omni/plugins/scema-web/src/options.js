/**
 * Pairing.
 *
 * Two things worth noting, both learned in `web/lib/net.ts` and worth not relearning:
 *
 *   1. **The stored base is the API root, and callers append their own path.** A trailing
 *      slash, or a pasted `.../health`, produces a doubled or wrong URL on every request.
 *      Normalised on write *and* on read, so an old pairing starts working again without a
 *      re-pair.
 *   2. **The probe demands JSON on a path with no alias.** `/health` answering 200 with
 *      HTML is a tunnel login page, not a daemon, and treating a 200 as success is how a
 *      wrong base URL is validated as a good one.
 */

const $ = (id) => document.getElementById(id);

/**
 * Apply the shared palette.
 *
 * `:host` only means anything inside a shadow root; in an ordinary document it matches
 * nothing, so the reset it carries would be silently dropped and the page would inherit the
 * browser's defaults for everything the sheet does not name.
 */
function applyTheme() {
  const style = document.createElement('style');
  style.textContent = globalThis.ScemaTheme.css().replace(
    ':host { all: initial; }',
    ':root { color-scheme: dark; }'
  );
  document.head.appendChild(style);
}

/** Strip trailing slashes and a pasted endpoint path. */
function normalizeBase(raw) {
  let s = (raw || '').trim();
  s = s.replace(/\/+$/, '');
  // Somebody will paste the URL they just curled.
  s = s.replace(/\/(health|policy|observe|simulate|decide|decisions|memory)$/i, '');
  return s;
}

async function load() {
  const stored = await chrome.storage.local.get(['baseUrl', 'token']);
  $('base').value = normalizeBase(stored.baseUrl) || 'http://127.0.0.1:7842';
  $('token').value = stored.token || '';
}

function report(cls, html) {
  const out = $('out');
  out.className = cls;
  out.innerHTML = html;
}

async function saveAndTest() {
  const baseUrl = normalizeBase($('base').value) || 'http://127.0.0.1:7842';
  const token = $('token').value.trim();
  $('base').value = baseUrl;
  await chrome.storage.local.set({ baseUrl, token });

  report('dim', 'testing…');

  // The probe goes through the service worker, so it exercises exactly the path a real
  // request takes. A test that used its own fetch could pass while the worker's failed.
  chrome.runtime.sendMessage({ type: 'health' }, (health) => {
    if (!health || !health.ok) {
      report(
        'invalid',
        `<strong>${health ? health.reason : 'no response'}</strong><br>` +
          `<span class="dim">${health ? health.detail : 'the service worker did not answer'}</span>`
      );
      return;
    }
    // Health takes no token, so a green health says nothing about the token. Ask for
    // something authenticated before claiming the pairing works.
    chrome.runtime.sendMessage({ type: 'policy' }, (policy) => {
      if (!policy || !policy.ok) {
        report(
          'invalid',
          `<strong>daemon reachable, token ${policy ? policy.reason : 'untested'}</strong><br>` +
            `<span class="dim">${policy ? policy.detail : ''}</span>`
        );
        return;
      }
      const p = policy.data;
      report(
        'valid',
        `<strong>paired</strong> — ${health.data.runtime}<br>` +
          `<span class="dim">workspace: ${(p.workspace_roots || []).join(', ')}<br>` +
          `decide over HTTP: ${p.allow_decide ? 'ON' : 'OFF'}<br>` +
          `evaluators: ${(p.evaluators || []).map((e) => e.name).join(', ')}</span>`
      );
    });
  });
}

$('save').addEventListener('click', saveAndTest);
// Enter in either field saves. A settings page with one button that the operator has to
// find with the mouse after typing a token is a settings page people abandon half-done.
['base', 'token'].forEach((id) =>
  $(id).addEventListener('keydown', (e) => {
    if (e.key === 'Enter') saveAndTest();
  })
);
applyTheme();
load();
