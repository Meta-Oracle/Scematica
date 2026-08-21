/**
 * The toolbar popup: pairing state, one button that perceives, and the record log.
 *
 * ## Why a popup replaced the bare toolbar click
 *
 * With `action.default_popup` set, `chrome.action.onClicked` no longer fires — so this file
 * owns the injection that `background.js` used to do. That is a trade and it is worth
 * naming: one click became two. What it buys is that the extension can now *say something*
 * before it reads anything. Before, a broken pairing surfaced as an overlay that appeared,
 * said `unreachable`, and left the operator guessing whether the daemon was down, the token
 * was wrong or the URL was; the diagnosis lived in an overlay injected into somebody's
 * banking page. Now the state is on screen before the page is touched.
 *
 * `activeTab` still governs the read. Opening the popup is a user invocation of the
 * extension, which is what grants it, and it grants access to exactly one tab for exactly
 * this interaction.
 *
 * ## It holds no credentials either
 *
 * Same rule as the content script, for a weaker but still real reason: the popup is an
 * extension page, so a hostile web page cannot read it — but it also has no need for the
 * token, and a surface that does not hold a secret cannot leak one. Every call goes through
 * the service worker by *message type*, and the worker maps the type to a path.
 */

const $ = (id) => document.getElementById(id);

/** The theme is a string, so the popup and the injected HUD cannot drift apart. */
function applyTheme() {
  const style = document.createElement('style');
  style.textContent = globalThis.ScemaTheme.css()
    // `:host` only means anything inside a shadow root. In an ordinary document it matches
    // nothing, so the reset it carries would be silently dropped.
    .replace(':host { all: initial; }', ':root { color-scheme: dark; }');
  document.head.appendChild(style);
}

function esc(s) {
  return String(s).replace(/[&<>"']/g, (c) =>
    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])
  );
}

function truncate(s, n) {
  const t = String(s);
  return t.length <= n ? t : t.slice(0, n - 1) + '…';
}

/** Promise wrapper over the worker, so this file can read top to bottom. */
function ask(type, body) {
  return new Promise((resolve) => {
    chrome.runtime.sendMessage({ type, body }, (res) =>
      resolve(res || { ok: false, reason: 'no_worker', detail: 'the service worker did not answer' })
    );
  });
}

// ── the current tab ───────────────────────────────────────────────────────────

async function currentTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return tab;
}

/**
 * Pages Chrome refuses to inject into: its own settings, the Web Store, PDFs, `view-source`.
 *
 * Checked up front and reported plainly. Letting the click fail produces a
 * `chrome.runtime.lastError` that the operator never sees, and a button that does nothing is
 * indistinguishable from a broken extension.
 */
function injectable(url) {
  if (!url) return { ok: false, why: 'this tab has no address yet' };
  if (/^(chrome|edge|about|devtools|view-source|moz-extension|chrome-extension):/i.test(url)) {
    return { ok: false, why: 'the browser does not allow extensions to read its own pages' };
  }
  if (/^https:\/\/chromewebstore\.google\.com/i.test(url) || /^https:\/\/chrome\.google\.com\/webstore/i.test(url)) {
    return { ok: false, why: 'the browser blocks extensions on the Web Store' };
  }
  if (/^file:/i.test(url)) {
    return { ok: false, why: 'file:// pages need "Allow access to file URLs" in the extension details' };
  }
  return { ok: true };
}

async function showPage() {
  const tab = await currentTab();
  const url = tab && tab.url;
  const can = injectable(url);
  let host = '—';
  try {
    host = new URL(url).host || url;
  } catch {
    host = url || '—';
  }
  $('page').innerHTML = can.ok
    ? `<span class="measured">${esc(truncate(host, 44))}</span>`
    : `<span class="abstained">${esc(truncate(host, 44))}</span>
       <div class="note abstained">Cannot perceive: ${esc(can.why)}</div>`;
  $('perceive').disabled = !can.ok;
  return { tab, can };
}

// ── pairing ───────────────────────────────────────────────────────────────────

async function showStatus() {
  const health = await ask('health');
  const el = $('status');
  if (!health.ok) {
    // Distinct reasons, distinctly rendered. "It failed" sends nobody anywhere; this is the
    // mistake `/mesh` made when every failure collapsed into "No instance paired".
    el.textContent = health.reason;
    el.className = 'invalid';
    el.title = health.detail || '';
    $('policy').innerHTML = `<span class="abstained">${esc(health.detail || '')}</span>`;
    return false;
  }
  el.textContent = health.data.runtime;
  el.className = 'measured';

  // Health takes no token, so a green health says nothing about the token. Ask for
  // something authenticated before claiming the pairing works.
  const policy = await ask('policy');
  if (!policy.ok) {
    $('policy').innerHTML =
      `<span class="abstained">daemon reachable, token ${esc(policy.reason)}</span>` +
      `<div class="note">${esc(policy.detail || '')}</div>`;
    return false;
  }
  const p = policy.data;
  const w = p.weights || {};
  $('policy').innerHTML = `
    <div><span class="k">workspace</span>
      <span class="measured">${esc((p.workspace_roots || []).join(', ') || '—')}</span></div>
    <div><span class="k">seal over HTTP</span>
      <span class="${p.allow_decide ? 'chosen' : 'label'}">${p.allow_decide ? 'ON' : 'off'}</span></div>
    <div><span class="k">lambda</span>
      <span class="measured">K ${Number(w.risk).toFixed(2)} &middot; C ${Number(w.cost).toFixed(2)}
      &middot; U ${Number(w.uncertainty).toFixed(2)} &middot; V ${Number(w.reversibility).toFixed(2)}</span></div>
    <div><span class="k">specialists</span>
      <span class="label">${esc((p.evaluators || []).map((e) => e.name).join(', ') || 'none')}</span></div>`;
  return true;
}

// ── the record log ────────────────────────────────────────────────────────────

async function showDecisions() {
  const res = await ask('decisions');
  const box = $('decisions');
  if (!res.ok) {
    box.innerHTML = `<span class="dim">${esc(res.detail || res.reason)}</span>`;
    return;
  }
  const rows = Array.isArray(res.data) ? res.data : [];
  if (!rows.length) {
    box.innerHTML =
      '<span class="dim">none sealed yet — this daemon has decided nothing</span>';
    return;
  }
  box.innerHTML = rows
    .slice(0, 8)
    .map((r) => {
      // Three states. An unreadable record is neither a valid one nor an invalid one, and
      // rendering it as invalid would be an accusation about a file nobody could open.
      if (r.unreadable) {
        return `<div><span class="abstained">UNREAD</span>
          <span class="dim"> ${esc(r.id)} — ${esc(truncate(r.unreadable, 40))}</span></div>`;
      }
      const outcome = r.chosen
        ? `<span class="chosen">chose ${esc(truncate(r.chosen, 26))}</span>`
        : `<span class="abstained">abstained — ${esc(truncate(r.abstained || '', 34))}</span>`;
      const cov = r.coverage ? `${r.coverage.measured}/${r.coverage.total}` : '?';
      return `<div style="margin-bottom:5px">
        <span class="measured">${esc(r.id)}</span>
        <span class="dim"> ${esc(truncate(r.goal || '', 30))}</span>
        <div style="padding-left:2px">${outcome}
          <span class="dim"> &middot; measured ${esc(cov)}</span>
          <button data-id="${esc(r.id)}" class="export" style="padding:0 6px;margin-left:4px">export</button>
        </div></div>`;
    })
    .join('');

  box.querySelectorAll('.export').forEach((b) =>
    b.addEventListener('click', () => exportRecord(b.getAttribute('data-id')))
  );
}

/**
 * Fetch one record and hand it to the operator as a file.
 *
 * Deliberately an export and **not** an in-extension verifier. Recomputing the commitment
 * here would mean a fourth implementation of the canonical encoding — after `canonical.rs`,
 * `web/lib/omni/canonical.ts` and its test fixture — and this workspace's own rule is that a
 * copy which drifts is worse than no copy. One differing byte and this popup would report an
 * untampered record as INVALID, which is the most damaging failure available: it teaches the
 * reader to stop believing the verifier.
 *
 * So the record leaves as bytes, and it is checked by something that already has a tested
 * implementation: `scema verify --file`, or the `/omni` page, which hashes it in the
 * reader's own browser and talks to nothing.
 */
async function exportRecord(id) {
  const res = await ask('record', { id });
  if (!res.ok) {
    $('decisions').insertAdjacentHTML(
      'beforeend',
      `<div class="note invalid">could not read ${esc(id)}: ${esc(res.detail || res.reason)}</div>`
    );
    return;
  }
  // Two spaces, matching `RecordStore::save`, so an exported file and the one on disk are
  // byte-identical and either can be handed to `scema verify --file`.
  const text = JSON.stringify(res.data, null, 2);
  const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
  await chrome.downloads.download({ url, filename: `scema-record-${id}.json`, saveAs: true });
  // Revoked on a timer rather than immediately: `downloads.download` resolves when the
  // download is *queued*, and revoking a blob URL the download has not read yet cancels it.
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}

// ── wiring ────────────────────────────────────────────────────────────────────

$('options').addEventListener('click', () => chrome.runtime.openOptionsPage());

$('perceive').addEventListener('click', async () => {
  const { tab, can } = await showPage();
  if (!can.ok || !tab || !tab.id) return;
  try {
    await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      files: ['src/theme.js', 'src/perceive.js', 'src/content.js'],
    });
    window.close();
  } catch (e) {
    $('page').insertAdjacentHTML(
      'beforeend',
      `<div class="note invalid">could not inject: ${esc(e.message)}</div>`
    );
  }
});

applyTheme();
showPage();
showStatus().then((paired) => {
  if (paired) showDecisions();
  else $('decisions').innerHTML = '<span class="dim">not paired</span>';
});
