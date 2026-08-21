/**
 * The HUD: a cognitive overlay, not a chat window.
 *
 * Injected on demand from the popup button or the keyboard command (see `background.js`),
 * it perceives the page, shows what it could and could not see, and offers to run the omni
 * loop against a goal.
 *
 * ## The render rule travels with the data
 *
 * `scema_policy::render::cell` is the only thing in Rust allowed to format a `Term`, and an
 * unmeasured one prints `—` rather than `0.00`. That rule is not a property of a terminal;
 * it is a property of the claim. So `cell()` below is the same function, and the matrix
 * here is the same matrix. A column of numbers is the most persuasive thing a program can
 * put on a screen, and the moment an unmeasured neutral element renders as a number, the
 * distinction the whole Rust type system has been protecting is gone in the last hundred
 * lines of the product.
 *
 * ## Everything is in a closed shadow root
 *
 * With an `all: initial` reset, so the host page's CSS cannot restyle the HUD and the HUD
 * cannot restyle the page. A panel that inherited `* { display: none }` from some site's
 * reset would look like the extension failing.
 *
 * The styling comes from `theme.js` — the same palette the console draws with, ported from
 * `crates/scema-tui/src/theme.rs`. Before that file existed each surface carried its own
 * hex values and they had already drifted.
 *
 * ## The overlay holds no credentials
 *
 * It sends `{type, body}` to the service worker and gets a result back. It never sees the
 * token or the daemon URL, and it cannot name a path. See the note at the top of
 * `background.js`.
 *
 * ## It does not verify anything itself
 *
 * The commitment status shown for a sealed record is **the daemon's**, labelled as such.
 * Recomputing it here would mean a fourth implementation of the canonical encoding, and
 * this workspace's rule is that a copy which drifts is worse than no copy: one differing
 * byte and the overlay reports an untampered record as INVALID, which teaches the reader to
 * stop believing the verifier. Export instead, and check it with `scema verify --file` or
 * the `/omni` page.
 */

(() => {
  const HOST_ID = 'scema-omni-hud-host';
  const existing = document.getElementById(HOST_ID);
  if (existing) {
    existing.remove();
    return; // invoking again closes it
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
    return term.measured
      ? `<span class="measured">${term.value.toFixed(2)}</span>`
      : '<span class="unmeasured">—</span>';
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

  function coverageLabel(c) {
    return c ? `${c.measured}/${c.total}` : '?';
  }

  /**
   * One cell per term, filled or hollow.
   *
   * A count, not a percentage bar — the same widget the console draws, and for the same
   * reason: 2/5 and 4/10 are the same fraction and different claims, and a proportional bar
   * erases the denominator, which is the number that matters.
   */
  function coverageMeter(c) {
    if (!c || c.total === 0) return '<span class="dim">∅</span>';
    if (c.total > 12) return `<span class="dim">${c.measured}/${c.total}</span>`;
    return (
      '<span class="measured">' +
      '▰'.repeat(c.measured) +
      '</span><span class="unmeasured">' +
      '▱'.repeat(c.total - c.measured) +
      '</span>'
    );
  }

  /** The signal tag, which carries the meaning when the colour is gone. */
  function signalTag(s) {
    if (!s.measured) return 'EST?';
    return s.polarity === 'risk' ? 'RISK' : 'OPP ';
  }

  function signalClass(s) {
    if (!s.measured) return 'estimated';
    return s.polarity === 'risk' ? 'risk' : 'opportunity';
  }

  // ── shell ──────────────────────────────────────────────────────────────────

  const host = document.createElement('div');
  host.id = HOST_ID;
  host.style.cssText =
    'all: initial; position: fixed; top: 16px; right: 16px; z-index: 2147483647;';
  const root = host.attachShadow({ mode: 'closed' });

  const panelStyle = `
    .omni { width: 460px; max-height: 82vh; overflow: auto; }
  `;

  root.innerHTML = `
    <style>${globalThis.ScemaTheme.css()}${panelStyle}</style>
    <div class="omni">
      <div class="bar">
        <b>SCEMA OMNI</b><span class="sp"></span>
        <span id="status" class="dim">checking…</span>
        <button id="close" title="close">✕</button>
      </div>

      <section>
        <h4>WORLD</h4>
        <div><span class="k">entity</span> ${esc(truncate(world.entity.locator, 44))}</div>
        <div><span class="k">extent</span>
          ${world.extent.total === null
            ? `<span class="abstained">${world.extent.observed} observed, EXTENT UNBOUNDED</span>`
            : `<span class="measured">${world.extent.observed} of ${world.extent.total} observed</span>`}
          <div class="note">${esc(world.extent.note)}</div></div>
        <div><span class="k">objects</span> <span class="measured">${world.objects.length}</span>
          &nbsp;<span class="k" style="min-width:0">signals</span>
          <span class="measured">${world.signals.length}</span></div>
        <div id="blind"></div>
      </section>

      <section id="signals"><h4>COUNTED SIGNALS</h4></section>

      <section>
        <h4>ASK</h4>
        <div class="row">
          <input id="goal" placeholder="what should be done about this page?" />
          <button id="run" class="primary">Simulate</button>
        </div>
        <div class="row" style="margin-top:6px">
          <button id="seal" title="Seals a decision record on the daemon">Decide &amp; seal…</button>
          <span class="sp"></span>
          <span class="note" style="margin:0">Simulate writes nothing.</span>
        </div>
        <div class="note">
          Grounding is <strong>not</strong> inferred from wording — tick a signal above to
          assert the goal addresses it. An ungrounded goal branch scores at or below zero
          and the agent abstains, which is the honest answer to an instruction the page says
          nothing about.
        </div>
      </section>

      <section id="out"></section>
    </div>
  `;

  document.documentElement.appendChild(host);
  const $ = (sel) => root.querySelector(sel);

  $('#close').addEventListener('click', () => host.remove());
  // Escape closes it. An overlay pinned over somebody's page with no obvious way out is a
  // thing people uninstall.
  root.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') host.remove();
  });

  // ── blind spots: the most useful thing this observer knows ────────────────
  if (world.blind_spots.length) {
    $('#blind').innerHTML =
      `<div class="abstained" style="margin-top:5px">${world.blind_spots.length} thing(s) could not be read</div><ul>` +
      world.blind_spots
        .slice(0, 5)
        .map((b) => `<li class="abstained">${esc(truncate(b, 62))}</li>`)
        .join('') +
      (world.blind_spots.length > 5
        ? `<li class="dim">… ${world.blind_spots.length - 5} more</li>`
        : '') +
      '</ul>';
  }

  // ── signals, each with a checkbox that grounds the goal ───────────────────
  const sigBox = $('#signals');
  if (!world.signals.length) {
    sigBox.insertAdjacentHTML(
      'beforeend',
      '<div class="abstained">none counted on this page — nothing can ground a branch, so the agent will abstain</div>'
    );
  } else {
    sigBox.insertAdjacentHTML(
      'beforeend',
      world.signals
        .map(
          (s) => `
        <label class="pick">
          <input type="checkbox" class="ground" value="${esc(s.id)}" />
          <span class="${signalClass(s)}">${esc(signalTag(s))}</span>
          <span class="${s.measured ? 'measured' : 'estimated'}">${s.magnitude.toFixed(2)}</span>
          ${esc(truncate(s.label, 46))}
          <div class="note">${esc(truncate(s.evidence[0] || '', 74))}</div>
        </label>`
        )
        .join('')
    );
  }

  // ── daemon status ─────────────────────────────────────────────────────────
  chrome.runtime.sendMessage({ type: 'health' }, (res) => {
    const el = $('#status');
    if (res && res.ok) {
      el.textContent = res.data.runtime;
      el.className = 'measured';
    } else {
      el.textContent = res ? res.reason : 'no worker';
      el.className = 'invalid';
      el.title = res ? res.detail : '';
    }
  });

  // ── running ───────────────────────────────────────────────────────────────

  function groundedIds() {
    return Array.from(root.querySelectorAll('.ground'))
      .filter((c) => c.checked)
      .map((c) => c.value);
  }

  function runCycle(type) {
    const buttons = [$('#run'), $('#seal')];
    buttons.forEach((b) => (b.disabled = true));
    const out = $('#out');
    out.innerHTML = `<span class="dim">${type === 'decide' ? 'deciding' : 'simulating'}…</span>`;

    chrome.runtime.sendMessage(
      { type, body: { world, goal: $('#goal').value, ground: groundedIds() } },
      (res) => {
        buttons.forEach((b) => (b.disabled = false));
        if (!res || !res.ok) {
          // Distinct reasons, distinctly rendered. "It failed" sends nobody anywhere.
          out.innerHTML =
            `<h4>NOT RUN</h4><div class="invalid">${esc(res ? res.reason : 'no response')}</div>` +
            `<div class="note">${esc(res ? res.detail : '')}</div>` +
            (res && res.code === 'decide_disabled'
              ? '<div class="note">The daemon was started without <code>--allow-decide</code>. That is the default: sealing a record is a local write, and a front end that can be driven by a page should have to be told it may.</div>'
              : '');
          return;
        }
        out.innerHTML = renderCycle(res.data);
        const exportBtn = root.querySelector('#export');
        if (exportBtn) {
          exportBtn.addEventListener('click', () => exportRecord(res.data.record));
        }
      }
    );
  }

  $('#run').addEventListener('click', () => runCycle('simulate'));

  /**
   * Confirming a seal, inside the shadow root.
   *
   * **Not** `window.confirm`. A content script shares the page's window, so `confirm` is
   * whatever the page last assigned to it — a hostile page can define
   * `window.confirm = () => true` and the dialog the operator never saw returns yes. The
   * confirmation is the only thing standing between a counterfactual and a sealed record,
   * so it has to live somewhere the page cannot reach, which is this closed shadow root.
   *
   * The daemon still refuses `/decide` without `--allow-decide`, so this is defence in
   * depth rather than the only gate. It is worth having anyway: the operator who *did*
   * enable it is exactly the one whose confirmation now matters.
   */
  $('#seal').addEventListener('click', () => {
    const open = $('#seal-confirm');
    if (open) {
      open.remove();
      return;
    }
    const warning = groundedIds().length
      ? ''
      : '<div class="abstained note">Nothing is ticked as a ground, so this will almost certainly abstain — and the abstention is sealed as a record all the same.</div>';
    $('#seal').insertAdjacentHTML(
      'afterend',
      `<div id="seal-confirm" style="margin-top:6px">
         <div class="note">Seal a decision record on the daemon? It writes under
           <code>.scema/decisions/</code> and appends one counterfactual per branch not taken.</div>
         <div><span class="k">goal</span>${esc(truncate($('#goal').value || '(empty)', 42))}</div>
         ${warning}
         <div class="row" style="margin-top:5px">
           <button id="seal-yes" class="primary">Seal it</button>
           <button id="seal-no">Cancel</button>
         </div>
       </div>`
    );
    $('#seal-yes').addEventListener('click', () => {
      $('#seal-confirm').remove();
      runCycle('decide');
    });
    $('#seal-no').addEventListener('click', () => $('#seal-confirm').remove());
  });

  /**
   * Hand the record to the operator as a file.
   *
   * Two spaces, matching `RecordStore::save`, so an exported file is byte-identical to the
   * one on disk and either can be given to `scema verify --file`.
   */
  function exportRecord(record) {
    const blob = new Blob([JSON.stringify(record, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `scema-record-${record.id}.json`;
    a.style.display = 'none';
    // Appended to the page's own document rather than to the shadow root: a click on a
    // detached anchor does nothing in Chrome, and the download would silently not happen.
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  }

  function renderCycle(payload) {
    const rec = payload.record;
    const d = rec.decision;
    const byId = Object.fromEntries(rec.projections.map((p) => [p.hypothesis, p]));

    const rows = d.ranked
      .map((r) => {
        const p = byId[r.hypothesis];
        const win = d.chosen === r.hypothesis;
        return `<tr>
          <td class="b ${win ? 'chosen' : ''}">${win ? '▸ ' : ''}${esc(truncate(r.statement, 40))}</td>
          <td>${cell(p && p.expected_gain)}</td>
          <td>${cell(p && p.risk)}</td>
          <td>${cell(p && p.cost)}</td>
          <td>${cell(p && p.uncertainty)}</td>
          <td>${cell(p && p.reversibility)}</td>
          <td class="${win ? 'chosen' : ''}">${r.utility.value.toFixed(3)}</td>
          <td>${coverageMeter(r.utility.coverage)}</td>
        </tr>`;
      })
      .join('');

    const excluded = d.excluded
      .map(
        (e) =>
          `<tr><td class="b excluded">${esc(truncate(e.statement, 40))}</td>
           <td colspan="7" class="b abstained">EXCLUDED — ${esc(truncate(e.reason, 44))}</td></tr>`
      )
      .join('');

    const verdict = d.chosen
      ? `<div><span class="k">DECISION</span> <span class="chosen">${esc(d.chosen)}</span></div>`
      : `<div><span class="k">ABSTAINED</span> <span class="abstained">${esc(
          d.abstention ? headline(d.abstention) : 'no reason recorded'
        )}</span></div>
         <div class="note">${esc(d.abstention ? advice(d.abstention) : '')}</div>`;

    const declining = d.evaluator_status
      .filter((s) => s.applicability.kind !== 'applicable')
      .map(
        (s) =>
          `<li class="dim">${esc(s.evaluator)} — ${esc(s.applicability.kind)}: ${esc(
            truncate(s.applicability.note, 70)
          )}</li>`
      )
      .join('');

    const dangling = payload.dangling_grounds.length
      ? `<div class="abstained note">ignored grounding: ${payload.dangling_grounds
          .map(esc)
          .join(', ')} — no such signal in this world</div>`
      : '';

    const trace = payload.persisted
      ? `<div class="note"><span class="valid">SEALED</span> as
           <code>${esc(rec.id)}</code>
           <button id="export" style="margin-left:6px;padding:1px 7px">export</button>
           <div style="margin-top:4px">The commitment status you will see elsewhere is
           <em>the daemon's</em>. This overlay recomputed nothing — check it with
           <code>scema verify ${esc(rec.id)}</code>, or drop the exported file on the
           <code>/omni</code> page, which hashes it in your own browser.</div></div>`
      : `<div class="note">not written — a counterfactual leaves no trace. Would seal as
           <code>${esc(rec.id)}</code>.</div>`;

    return `
      <h4>SIMULATION MATRIX</h4>
      <table>
        <tr><th class="b">BRANCH</th><th>GAIN</th><th>RISK</th><th>COST</th>
            <th>UNC</th><th>REV</th><th>U</th><th>MEAS</th></tr>
        ${rows}${excluded}
      </table>
      <div class="note">measured across the matrix: ${coverageLabel(d.coverage)} —
        “—” means the term was not measured and contributed nothing to the utility beside it.</div>
      ${dangling}
      <div style="margin-top:8px">${verdict}</div>
      ${declining ? `<div class="note">specialists that declined:<ul>${declining}</ul></div>` : ''}
      ${trace}`;
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
        return `the ranking stands on ${a.coverage.measured}/${a.coverage.total} measured term(s)`;
      case 'contested':
        return `${a.by} is qualified here and scores the top branch ${a.utility.toFixed(3)}`;
      default:
        return a.reason;
    }
  }

  /**
   * Mirror of `view::abstention_advice` in the console.
   *
   * Five reasons, five different places to send the operator. Collapsing them into "the
   * agent declined" throws away the only actionable part of an abstention.
   */
  function advice(a) {
    switch (a.reason) {
      case 'no_candidates':
        return 'Nothing was proposed. This page produced no counted signals to build a branch from.';
      case 'all_forbidden':
        return 'Every branch violates a constraint on the goal. The goal is unsatisfiable as stated.';
      case 'no_positive_utility':
        return 'Acting scores worse than not acting. Accept that, or lower the bar deliberately in policy.';
      case 'too_little_measured':
        return 'This is a statement about how little was observed, not about the branches. Go and observe more.';
      case 'contested':
        return 'A specialist that IS qualified here disagrees with the top branch. Read its note before overriding it.';
      default:
        return '';
    }
  }
})();
