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
  assert.equal(w.domain, 'unknown', 'guessing a page domain would be a guess');
  assert.ok(w.objects.some((o) => o.id === 'document'));
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
