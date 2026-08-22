/**
 * Tests for page perception, on a hand-built fake document.
 *
 * No jsdom, no bundler, no dependencies — `node --test test/`. The fake implements the
 * narrow slice of the DOM `perceive` actually touches (`querySelectorAll`, `getAttribute`,
 * `title`), which is a useful constraint in itself: a perception function that needs a real
 * browser to test is one nobody tests.
 */

const test = require('node:test');
const assert = require('node:assert');
const { perceive, originOf } = require('../src/perceive.js');

/** A fake element. `sel` matching is by tag plus the handful of attribute selectors used. */
function el(tag, attrs = {}, children = []) {
  return {
    tag: tag.toLowerCase(),
    attrs,
    children,
    getAttribute(name) {
      return Object.prototype.hasOwnProperty.call(this.attrs, name) ? this.attrs[name] : null;
    },
    querySelectorAll(sel) {
      return matchAll(flatten(this.children), sel);
    },
  };
}

function flatten(nodes) {
  const out = [];
  for (const n of nodes) {
    out.push(n);
    out.push(...flatten(n.children || []));
  }
  return out;
}

function matches(node, sel) {
  const s = sel.trim();
  if (s === '*') return true;
  // `[attr]` with no tag. Needed for the inline-event-handler scan, which is a list of
  // bare attribute selectors — the alternative would be enumerating every element type
  // that can carry an `onclick`, which is all of them.
  const bareAttr = s.match(/^\[([\w-]+)(?:=(.+))?\]$/);
  if (bareAttr) {
    const [, name, value] = bareAttr;
    const v = node.getAttribute(name);
    if (v === null) return false;
    return value === undefined || v === value;
  }
  const withAttr = s.match(/^(\w+)\[([\w-]+)(?:=(.+))?\]$/);
  if (withAttr) {
    const [, tag, name, value] = withAttr;
    if (node.tag !== tag) return false;
    const v = node.getAttribute(name);
    if (v === null) return false;
    return value === undefined || v === value;
  }
  return node.tag === s;
}

function matchAll(nodes, sel) {
  const parts = sel.split(',').map((s) => s.trim());
  return nodes.filter((n) => parts.some((p) => matches(n, p)));
}

function doc(children, title = 'Test page') {
  const flat = flatten(children);
  return {
    title,
    querySelectorAll(sel) {
      return matchAll(flat, sel);
    },
  };
}

const httpsLoc = { href: 'https://example.test/page', origin: 'https://example.test', protocol: 'https:', pathname: '/page' };
const httpLoc = { href: 'http://example.test/page', origin: 'http://example.test', protocol: 'http:', pathname: '/page' };

test('a clean page produces no signals but still describes itself', () => {
  const w = perceive(doc([el('p')]), httpsLoc, 0);
  assert.equal(w.signals.length, 0);
  assert.equal(w.observer, 'page');
  // `web`, and this is not the guess the earlier `unknown` was avoiding. Two different
  // questions were being run together: what a page is *about* is unknowable from a DOM and
  // must not be guessed, but what kind of world this is has one answer and the producer
  // knows it — it only ever perceives pages. `unknown` reported the same thing an oracle
  // set reported, so nothing downstream could tell the two apart. No specialist gains a
  // claim of competence from this; they match the arm they understand and decline.
  assert.equal(w.domain, 'web');
  assert.ok(w.objects.some((o) => o.id === 'document'));
});

test('the world declares the contract version it was written against', () => {
  // The one rule no producer can enforce for the others. `perceive.js` cannot link
  // `scema-world`, so this string is the only thing telling an importer which reading of
  // the format these bytes were built against — and without it the next change to the
  // format is a silent misread rather than an error message.
  const w = perceive(doc([]), httpsLoc, 0);
  assert.equal(w.schema, 'scema.world/1');
});

test('the locator drops the query string and fragment', () => {
  // It is hashed into a decision record that outlives the tab, and query strings routinely
  // carry session tokens and search terms.
  const loc = { ...httpsLoc, href: 'https://example.test/page?token=secret#x' };
  const w = perceive(doc([]), loc, 0);
  assert.equal(w.locator, undefined);
  assert.equal(w.entity.locator, 'https://example.test/page');
  assert.ok(!JSON.stringify(w).includes('secret'));
});

test('a cross-origin frame becomes a blind spot, not an empty frame', () => {
  // The obligation that matters most in a browser: the same-origin policy makes this
  // genuinely unreadable, and reporting it as "no forms found" would be a false statement.
  const w = perceive(doc([el('iframe', { src: 'https://other.test/widget' })]), httpsLoc, 0);
  assert.equal(w.blind_spots.length, 1);
  assert.match(w.blind_spots[0], /cross-origin/);
  assert.match(w.blind_spots[0], /unreadable/);
});

test('a same-origin frame is not a blind spot', () => {
  const w = perceive(doc([el('iframe', { src: '/inner' })]), httpsLoc, 0);
  assert.deepEqual(w.blind_spots, []);
});

test('every signal is measured and cites a count', () => {
  // The property `scema-sim` depends on to score an expected gain at all.
  const w = perceive(
    doc([
      el('form', { action: 'http://insecure.test/submit' }, [el('input', { type: 'password' })]),
      el('img', {}),
      el('script', { src: 'https://cdn.test/a.js' }),
      el('a', { href: 'https://other.test', target: '_blank' }),
    ]),
    httpsLoc,
    0
  );
  assert.ok(w.signals.length > 0);
  for (const s of w.signals) {
    assert.equal(s.measured, true, `${s.id} must be measured`);
    assert.ok(s.evidence.length > 0, `${s.id} must cite evidence`);
    assert.match(s.evidence[0], /counted/, `${s.id} evidence must be a count`);
    assert.ok(s.magnitude >= 0 && s.magnitude <= 1, `${s.id} magnitude in range`);
  }
});

test('a password field on an http page is a full-magnitude risk', () => {
  const w = perceive(doc([el('form', {}, [el('input', { type: 'password' })])]), httpLoc, 0);
  const s = w.signals.find((x) => x.id === 'password-on-insecure-page');
  assert.ok(s, 'expected the signal');
  assert.equal(s.magnitude, 1);
});

test('the same page over https does not raise that risk', () => {
  const w = perceive(doc([el('form', {}, [el('input', { type: 'password' })])]), httpsLoc, 0);
  assert.equal(w.signals.find((x) => x.id === 'password-on-insecure-page'), undefined);
});

test('third-party script origins are counted distinctly, not per tag', () => {
  const w = perceive(
    doc([
      el('script', { src: 'https://cdn.test/a.js' }),
      el('script', { src: 'https://cdn.test/b.js' }),
      el('script', { src: 'https://other.test/c.js' }),
      el('script', { src: '/local.js' }),
    ]),
    httpsLoc,
    0
  );
  const s = w.signals.find((x) => x.id === 'third-party-scripts');
  assert.match(s.label, /2 third-party script origin/);
  assert.match(
    s.evidence[0],
    /counted 2 distinct third-party origin\(s\) across 3 off-origin script tag\(s\), of 4 with a src/
  );
});

test('an unbounded scan reports total as null rather than claiming completeness', () => {
  // The same rule as `Extent { total: None }` in Rust: a capped walk does not know what it
  // missed, and `total === observed` would claim it saw everything.
  const many = [];
  for (let i = 0; i < 5100; i += 1) many.push(el('div'));
  const w = perceive(doc(many), httpsLoc, 0);
  assert.equal(w.extent.total, null);
  assert.match(w.extent.note, /capped/);

  const small = perceive(doc([el('div')]), httpsLoc, 0);
  assert.equal(small.extent.total, small.extent.observed);
});

test('objects carry live provenance and never a bare zero for something unseen', () => {
  const w = perceive(doc([el('iframe', { src: 'https://other.test/x' })]), httpsLoc, 0);
  const group = w.objects.find((o) => o.id === 'frames:cross-origin');
  assert.equal(group.attrs.frames.v, 1, 'we can count that the frame exists');
  assert.equal(
    Object.prototype.hasOwnProperty.call(group.attrs, 'forms'),
    false,
    'we cannot see inside it, so there is no forms count at all'
  );
});

test('an unresolvable url yields no origin rather than throwing', () => {
  // With no base, `new URL` genuinely fails.
  assert.equal(originOf('::::'), null);
});

test('a nonsense src resolves as a relative path and is not third-party', () => {
  // `new URL(src, base)` resolves almost anything relative rather than throwing, so `::::`
  // lands on this page's own origin. That is the correct answer and worth pinning: an
  // earlier reading of this assumed it would fail to parse, which would have made a
  // same-origin script look like an unknown third party.
  assert.equal(originOf('::::', 'https://example.test/page'), 'https://example.test');
  const w = perceive(doc([el('script', { src: '::::' })]), httpsLoc, 0);
  assert.equal(w.signals.find((x) => x.id === 'third-party-scripts'), undefined);
});

test('the shape matches what the Rust WorldState deserialiser expects', () => {
  // A field renamed on one side and not the other fails as a 400 from the daemon with a
  // serde message, which is a slow way to find out. Checked structurally here instead.
  const w = perceive(doc([el('form', {}, [el('input', {})])]), httpsLoc, 123);
  for (const key of ['observer', 'entity', 'domain', 'observed_at', 'objects', 'facts', 'signals', 'extent', 'blind_spots']) {
    assert.ok(Object.prototype.hasOwnProperty.call(w, key), `missing ${key}`);
  }
  assert.deepEqual(Object.keys(w.entity).sort(), ['kind', 'label', 'locator']);
  assert.deepEqual(Object.keys(w.extent).sort(), ['note', 'observed', 'total']);
  const o = w.objects[0];
  assert.deepEqual(Object.keys(o).sort(), ['attrs', 'id', 'kind', 'label', 'provenance']);
  assert.equal(o.provenance.kind, 'live');
  // Scalars are the externally-tagged `{t, v}` form `scema_world::Scalar` declares.
  const scalar = Object.values(o.attrs)[0];
  assert.ok(['int', 'num', 'text', 'bool'].includes(scalar.t));
  assert.equal(w.observed_at, 123);
});

// ── signals added in 0.2.0 ────────────────────────────────────────────────────
//
// Every one of these counts something. None of them estimates a severity, which is what
// lets `scema-sim` treat the magnitude as `measured` and score a real expected gain from
// it. A test that asserted on a "page quality score" would be pinning a hallucination.

test('a third-party script with an integrity hash is not counted as one without', () => {
  // Ten scripts from one pinned CDN and one script from an unpinned one are different
  // postures, and the origin count alone cannot tell them apart.
  const pinned = doc([
    el('script', { src: 'https://cdn.test/a.js', integrity: 'sha384-x' }),
    el('script', { src: 'https://cdn.test/b.js', integrity: 'sha384-y' }),
  ]);
  const w = perceive(pinned, httpsLoc, 0);
  assert.ok(!w.signals.some((s) => s.id === 'third-party-scripts-without-sri'));
  assert.ok(w.signals.some((s) => s.id === 'third-party-scripts'), 'the origin is still counted');

  const loose = doc([el('script', { src: 'https://cdn.test/a.js' })]);
  const sri = perceive(loose, httpsLoc, 0).signals.find(
    (s) => s.id === 'third-party-scripts-without-sri'
  );
  assert.ok(sri);
  assert.equal(sri.measured, true);
  assert.match(sri.evidence[0], /counted 1 of 1/);
});

test('mixed content is only counted on a page that is itself secure', () => {
  // An http subresource on an http page is not "mixed", it is consistent — and the
  // page-level signal already covers the real problem there. Counting it twice would
  // double it into the same decision.
  const children = [el('img', { src: 'http://cdn.test/a.png' })];
  assert.ok(
    perceive(doc(children), httpsLoc, 0).signals.some((s) => s.id === 'mixed-content-subresources')
  );
  assert.ok(
    !perceive(doc(children), httpLoc, 0).signals.some((s) => s.id === 'mixed-content-subresources')
  );
});

test('inline event handlers are counted, not judged', () => {
  const w = perceive(
    doc([el('button', { onclick: 'go()' }), el('div', { onmouseover: 'hi()' }), el('p')]),
    httpsLoc,
    0
  );
  const s = w.signals.find((x) => x.id === 'inline-event-handlers');
  assert.ok(s);
  assert.match(s.label, /2 element/);
  // The label says what was counted. The detail says why it matters. Neither claims the
  // page is insecure, because counting handlers does not establish that.
  assert.match(s.detail, /script-src/);
});

test('a javascript: link is counted separately from an ordinary one', () => {
  const w = perceive(
    doc([el('a', { href: 'javascript:void 0' }), el('a', { href: '/ok' })]),
    httpsLoc,
    0
  );
  const s = w.signals.find((x) => x.id === 'javascript-url-links');
  assert.ok(s);
  assert.match(s.evidence[0], /counted 1/);
});

test('a labelled control is not counted as unlabelled, by any of the four routes', () => {
  const labelled = doc([
    el('label', { for: 'a' }),
    el('input', { id: 'a', type: 'text' }),
    el('input', { 'aria-label': 'search', type: 'text' }),
    el('input', { 'aria-labelledby': 'h', type: 'text' }),
    el('input', { title: 'phone', type: 'text' }),
  ]);
  assert.ok(!perceive(labelled, httpsLoc, 0).signals.some((s) => s.id === 'controls-without-labels'));
});

test('buttons and hidden inputs are not form fields for this purpose', () => {
  // A submit button announces its own value; a hidden input announces nothing to anybody.
  // Counting either produces a number that a reader investigates and dismisses, which is
  // how a signal stops being read.
  const w = perceive(
    doc([
      el('input', { type: 'hidden', name: 'csrf' }),
      el('input', { type: 'submit', value: 'Go' }),
      el('input', { type: 'button' }),
    ]),
    httpsLoc,
    0
  );
  assert.ok(!w.signals.some((s) => s.id === 'controls-without-labels'));
});

test('the unlabelled-control count says which way it errs', () => {
  // A wrapping `<label><input></label>` is not detected, so this over-counts. The evidence
  // string has to say so: a number whose bias is undocumented is a number a reader cannot
  // calibrate against, and this one will be wrong in a predictable direction.
  const w = perceive(doc([el('input', { type: 'text' })]), httpsLoc, 0);
  const s = w.signals.find((x) => x.id === 'controls-without-labels');
  assert.ok(s);
  assert.match(s.evidence[0], /over-counts/);
});

test('heading level skips are counted in document order', () => {
  const w = perceive(
    doc([el('h1'), el('h3'), el('h4'), el('h6')]),
    httpsLoc,
    0
  );
  const s = w.signals.find((x) => x.id === 'heading-level-skips');
  assert.ok(s);
  // h1 -> h3 skips, h3 -> h4 does not, h4 -> h6 skips.
  assert.match(s.evidence[0], /counted 2 skip/);
});

test('a correct heading outline produces no signal at all', () => {
  const w = perceive(doc([el('h1'), el('h2'), el('h3'), el('h2')]), httpsLoc, 0);
  assert.ok(!w.signals.some((s) => s.id === 'heading-level-skips'));
});

test('every magnitude stays inside [0,1] however many things are counted', () => {
  // `signal()` clamps, and the clamp is what keeps a page with four hundred inline handlers
  // from producing a magnitude of ten and dominating a ranking through arithmetic rather
  // than through importance.
  const many = [];
  for (let i = 0; i < 400; i += 1) many.push(el('div', { onclick: 'x()' }));
  const w = perceive(doc(many), httpsLoc, 0);
  assert.ok(w.signals.every((s) => s.magnitude >= 0 && s.magnitude <= 1), JSON.stringify(w.signals));
});

test('every signal is measured and cites how it was counted', () => {
  // The property `scema-sim` depends on. An estimated magnitude that claimed to be counted
  // would launder a guess into an expected gain, and nothing downstream could tell.
  const w = perceive(
    doc([
      el('script', { src: 'https://cdn.test/a.js' }),
      el('img', { src: 'http://cdn.test/a.png' }),
      el('a', { href: 'javascript:void 0' }),
      el('input', { type: 'text' }),
      el('h1'),
      el('h3'),
      el('button', { onclick: 'x()' }),
      el('img', { src: '/b.png' }),
    ]),
    httpsLoc,
    0
  );
  assert.ok(w.signals.length >= 6, `expected several signals, got ${w.signals.length}`);
  for (const s of w.signals) {
    assert.equal(s.measured, true, `${s.id} must be counted, not estimated`);
    assert.ok(s.evidence.length > 0, `${s.id} must cite its count`);
    assert.match(s.evidence[0], /counted/, `${s.id}: ${s.evidence[0]}`);
  }
});

test('signal ids are unique within one perception', () => {
  // Two signals with one id would be ranked as two branches supporting the same thing, and
  // `--ground` could not name either of them unambiguously.
  const w = perceive(
    doc([
      el('script', { src: 'https://a.test/x.js' }),
      el('script', { src: 'https://b.test/y.js' }),
      el('img', { src: 'http://c.test/z.png' }),
      el('input', { type: 'text' }),
      el('select', {}),
    ]),
    httpsLoc,
    0
  );
  const ids = w.signals.map((s) => s.id);
  assert.equal(new Set(ids).size, ids.length, ids.join(', '));
});
