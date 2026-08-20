/**
 * The HUD: a cognitive overlay, not a chat window.
 *
 * Injected on demand by the toolbar click (see `background.js`), it perceives the page,
 * shows what it could and could not see, and offers to run the omni loop against a goal.
 *
 * ## The render rule travels with the data
 *
 * `scema-cli`'s `render::cell` is the only thing allowed to format a `Term`, and an
 * unmeasured one prints `—` rather than `0.00`. That rule is not a property of the terminal;
 * it is a property of the claim. So `cell()` below is the same function, and the matrix
 * here is the same matrix. A column of numbers is the most persuasive thing a program can
 * put on a screen, and the moment an unmeasured neutral element renders as a number, the
 * distinction the whole Rust type system has been protecting is gone in the last hundred
 * lines of the product.
 *
 * ## Everything is in a shadow root
 *
 * A closed shadow root with an all-initial reset, so the host page's CSS cannot restyle the
 * HUD and the HUD cannot restyle the page. A panel that inherits `* { display: none }` from
 * some site's reset would look like the extension failing.
 *
 * ## The overlay holds no credentials
 *
 * It sends `{type: 'simulate', body}` to the service worker and gets a result back. It never
 * sees the token or the daemon URL. See the note at the top of `background.js`.
 */

(() => {
  const HOST_ID = 'scema-omni-hud-host';
  const existing = document.getElementById(HOST_ID);
  if (existing) {
    existing.remove();
    return; // clicking again closes it
  }

  const world = globalThis.ScemaPerceive.perceive(
    document,
    location,
    Math.floor(Date.now() / 1000)
  );

  // ── rendering helpers ──────────────────────────────────────────────────────

  /** Format a `Term`. Unmeasured prints an em dash, never a zero. */
  function cell(term) {
    if (!term) return '?';
    return term.measured ? term.value.toFixed(2) : '—';
  }

  function esc(s) {
    return String(s).replace(/[&<>"']/g, (c) =>
      ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])
    );
  }

  function coverageLabel(c) {
    return c ? `${c.measured}/${c.total}` : '?';
  }

  // ── shell ──────────────────────────────────────────────────────────────────

  const host = document.createElement('div');
  host.id = HOST_ID;
  host.style.cssText = 'all: initial; position: fixed; top: 16px; right: 16px; z-index: 2147483647;';
  const root = host.attachShadow({ mode: 'closed' });

  root.innerHTML = `
    <style>
      :host { all: initial; }
      .panel {
        font: 12px/1.5 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
        width: 430px; max-height: 78vh; overflow: auto;
        background: #0b0b12; color: #e6e6f0;
        border: 1px solid #4c4cff; border-radius: 8px;
        box-shadow: 0 12px 40px rgba(0,0,0,.55);
      }
      .bar { display:flex; align-items:center; gap:8px; padding:8px 10px;
             background:#12121e; border-bottom:1px solid #242440; position:sticky; top:0; }
      .bar b { color:#8b8bff; letter-spacing:.08em; font-weight:600; }
      .bar .sp { flex:1 }
      button { font: inherit; background:#1b1b2e; color:#e6e6f0; border:1px solid #35355c;
               border-radius:4px; padding:3px 9px; cursor:pointer; }
      button:hover { border-color:#4c4cff; }
      button[disabled] { opacity:.5; cursor:default; }
      section { padding:9px 10px; border-bottom:1px solid #1c1c30; }
      h4 { margin:0 0 5px; font-size:10px; letter-spacing:.12em; color:#7a7a9c; font-weight:600; }
      .row { display:flex; gap:8px; }
      .k { color:#7a7a9c; }
      input { font: inherit; flex:1; background:#08080f; color:#e6e6f0;
              border:1px solid #35355c; border-radius:4px; padding:4px 7px; }
      table { width:100%; border-collapse:collapse; }
      td, th { text-align:right; padding:2px 3px; white-space:nowrap; }
      th { color:#7a7a9c; font-weight:500; font-size:10px; }
      td.b, th.b { text-align:left; white-space:normal; }
      tr.win td { color:#9dff9d; }
      .warn { color:#ffb86b; }
      .bad { color:#ff7b7b; }
      .dim { color:#7a7a9c; }
      ul { margin:3px 0 0; padding-left:15px; }
      li { margin:1px 0; }
      .note { font-size:10px; color:#7a7a9c; margin-top:5px; }
    </style>
    <div class="panel">
      <div class="bar">
        <b>SCEMA OMNI</b><span class="sp"></span>
        <span id="status" class="dim">checking…</span>
        <button id="close">✕</button>
      </div>
      <section>
        <h4>WORLD</h4>
        <div><span class="k">entity</span> ${esc(world.entity.locator)}</div>
        <div><span class="k">extent</span> ${world.extent.observed} element(s)
          ${world.extent.total === null ? '<span class="warn">— EXTENT UNBOUNDED</span>' : ''}
          <span class="dim">${esc(world.extent.note)}</span></div>
        <div><span class="k">objects</span> ${world.objects.length}
          &nbsp;<span class="k">signals</span> ${world.signals.length}</div>
        <div id="blind"></div>
      </section>
      <section id="signals"><h4>COUNTED SIGNALS</h4></section>
      <section>
        <h4>ASK</h4>
        <div class="row">
          <input id="goal" placeholder="what should be done about this page?" />
          <button id="run">Simulate</button>
        </div>
        <div class="note">Simulate writes nothing. Grounding is not inferred from wording —
          tick a signal to assert the goal addresses it.</div>
      </section>
      <section id="out"></section>
    </div>
  `;

  document.documentElement.appendChild(host);
  const $ = (sel) => root.querySelector(sel);

  $('#close').addEventListener('click', () => host.remove());

  // ── blind spots: the most useful thing this observer knows ────────────────
  if (world.blind_spots.length) {
    $('#blind').innerHTML =
      `<div class="warn">blind spots (${world.blind_spots.length})</div><ul>` +
      world.blind_spots.slice(0, 5).map((b) => `<li class="dim">${esc(b)}</li>`).join('') +
      '</ul>';
  }

  // ── signals, each with a checkbox that grounds the goal ───────────────────
  const sigBox = $('#signals');
  if (!world.signals.length) {
    sigBox.innerHTML += '<div class="dim">none counted on this page</div>';
  } else {
    sigBox.innerHTML +=
      world.signals
        .map(
          (s) => `
        <label style="display:block">
          <input type="checkbox" class="ground" value="${esc(s.id)}" />
          <span class="${s.polarity === 'risk' ? 'bad' : ''}">${esc(s.label)}</span>
          <span class="dim">${s.magnitude.toFixed(2)}</span>
          <div class="note">${esc(s.evidence[0] || '')}</div>
        </label>`
        )
        .join('');
  }

  // ── daemon status ─────────────────────────────────────────────────────────
  chrome.runtime.sendMessage({ type: 'health' }, (res) => {
    const el = $('#status');
    if (res && res.ok) {
      el.textContent = res.data.runtime;
      el.className = '';
    } else {
      el.textContent = res ? res.reason : 'no worker';
      el.className = 'bad';
      el.title = res ? res.detail : '';
    }
  });

  // ── run ───────────────────────────────────────────────────────────────────
  $('#run').addEventListener('click', () => {
    const btn = $('#run');
    btn.disabled = true;
    const out = $('#out');
    out.innerHTML = '<span class="dim">simulating…</span>';

    const ground = Array.from(root.querySelectorAll('.ground'))
      .filter((c) => c.checked)
      .map((c) => c.value);

    chrome.runtime.sendMessage(
      { type: 'simulate', body: { world, goal: $('#goal').value, ground } },
      (res) => {
        btn.disabled = false;
        if (!res || !res.ok) {
          // Distinct reasons, distinctly rendered. "It failed" sends nobody anywhere.
          out.innerHTML = `<h4>NOT RUN</h4><div class="bad">${esc(res ? res.reason : 'no response')}</div>
            <div class="note">${esc(res ? res.detail : '')}</div>`;
          return;
        }
        out.innerHTML = renderCycle(res.data);
      }
    );
  });

  function renderCycle(payload) {
    const rec = payload.record;
    const d = rec.decision;
    const byId = Object.fromEntries(rec.projections.map((p) => [p.hypothesis, p]));

    const rows = d.ranked
      .map((r) => {
        const p = byId[r.hypothesis];
        const win = d.chosen === r.hypothesis;
        return `<tr class="${win ? 'win' : ''}">
          <td class="b">${win ? '▸ ' : ''}${esc(r.statement)}</td>
          <td>${cell(p && p.expected_gain)}</td>
          <td>${cell(p && p.risk)}</td>
          <td>${cell(p && p.cost)}</td>
          <td>${cell(p && p.uncertainty)}</td>
          <td>${cell(p && p.reversibility)}</td>
          <td><b>${r.utility.value.toFixed(3)}</b></td>
          <td class="dim">${coverageLabel(r.utility.coverage)}</td>
        </tr>`;
      })
      .join('');

    const excluded = d.excluded
      .map(
        (e) =>
          `<tr><td class="b dim">${esc(e.statement)}</td><td colspan="7" class="b warn">EXCLUDED — ${esc(e.reason)}</td></tr>`
      )
      .join('');

    const verdict = d.chosen
      ? `<div><span class="k">DECISION</span> <b>${esc(d.chosen)}</b></div>`
      : `<div><span class="k">ABSTAINED</span> <span class="warn">${esc(
          d.abstention ? headline(d.abstention) : 'no reason recorded'
        )}</span></div>`;

    const declining = d.evaluator_status
      .filter((s) => s.applicability.kind !== 'applicable')
      .map(
        (s) =>
          `<li class="dim">${esc(s.evaluator)} — ${esc(s.applicability.kind)}: ${esc(s.applicability.note)}</li>`
      )
      .join('');

    const dangling = payload.dangling_grounds.length
      ? `<div class="warn note">ignored grounding: ${payload.dangling_grounds.map(esc).join(', ')} — no such signal in this world</div>`
      : '';

    return `
      <h4>SIMULATION MATRIX</h4>
      <table>
        <tr><th class="b">BRANCH</th><th>GAIN</th><th>RISK</th><th>COST</th><th>UNC</th><th>REV</th><th>U</th><th>MEAS</th></tr>
        ${rows}${excluded}
      </table>
      <div class="note">measured across the matrix: ${coverageLabel(d.coverage)} —
        <span class="dim">“—” means the term was not measured and contributed nothing.</span></div>
      ${dangling}
      <div style="margin-top:7px">${verdict}</div>
      ${declining ? `<div class="note">evaluators that declined:<ul>${declining}</ul></div>` : ''}
      <div class="note">${
        payload.persisted
          ? `sealed as ${esc(rec.id)}`
          : `not written — a counterfactual leaves no trace. Would seal as ${esc(rec.id)}.`
      }</div>`;
  }

  /** Mirror of `Abstention::headline` in Rust, for the five reasons. */
  function headline(a) {
    switch (a.reason) {
      case 'no_candidates':
        return 'no hypotheses were proposed';
      case 'all_forbidden':
        return `all ${a.count} branch(es) violate a constraint on the goal`;
      case 'no_positive_utility':
        return `the best branch scores ${a.best.toFixed(3)}; acting is worse than not acting`;
      case 'too_little_measured':
        return `the ranking stands on ${a.coverage.measured}/${a.coverage.total} measured term(s) — a statement about how little was observed, not about the branches`;
      case 'contested':
        return `${a.by} is qualified here and scores the top branch ${a.utility.toFixed(3)}`;
      default:
        return a.reason;
    }
  }
})();
