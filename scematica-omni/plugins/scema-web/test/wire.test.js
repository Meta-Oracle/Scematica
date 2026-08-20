/**
 * The wire contract: does a world perceived in the browser actually satisfy the Rust
 * deserialiser?
 *
 * `perceive.test.js` checks the shape structurally, which catches a renamed field. It
 * cannot catch a *semantic* mismatch — an enum whose tag representation differs, a
 * `null` where Rust wants an `Option` it does not have `#[serde(default)]` on — and the
 * symptom of those is a 400 from the daemon with a serde message, found by hand, weeks
 * later.
 *
 * So this test posts a real perceived world to a real `scema-omnid` and asserts on the
 * decision that comes back. It **skips** unless the daemon is pointed at explicitly, so
 * `npm test` stays hermetic and offline:
 *
 * ```console
 * $ scema-omnid --root /tmp/w/.scema --allow /tmp/w --port 7846 &
 * $ SCEMA_OMNID_URL=http://127.0.0.1:7846 \
 *   SCEMA_OMNID_TOKEN=$(cat /tmp/w/.scema/omnid.token) \
 *   node --test test/
 * ```
 */

const test = require('node:test');
const assert = require('node:assert');
const { perceive } = require('../src/perceive.js');

const BASE = process.env.SCEMA_OMNID_URL;
const TOKEN = process.env.SCEMA_OMNID_TOKEN;
const live = Boolean(BASE && TOKEN);

// ── the same minimal fake DOM as perceive.test.js ──────────────────────────────
function el(tag, attrs = {}, children = []) {
  return {
    tag: tag.toLowerCase(),
    attrs,
    children,
    getAttribute(n) {
      return Object.prototype.hasOwnProperty.call(this.attrs, n) ? this.attrs[n] : null;
    },
    querySelectorAll(s) {
      return matchAll(flatten(this.children), s);
    },
  };
}
function flatten(ns) {
  const o = [];
  for (const n of ns) {
    o.push(n);
    o.push(...flatten(n.children || []));
  }
  return o;
}
function matches(n, sel) {
  const s = sel.trim();
  if (s === '*') return true;
  const m = s.match(/^(\w+)\[([\w-]+)(?:=(.+))?\]$/);
  if (m) {
    const [, t, a, v] = m;
    if (n.tag !== t) return false;
    const g = n.getAttribute(a);
    if (g === null) return false;
    return v === undefined || g === v;
  }
  return n.tag === s;
}
function matchAll(ns, sel) {
  const ps = sel.split(',').map((x) => x.trim());
  return ns.filter((n) => ps.some((p) => matches(n, p)));
}

/** An insecure login page with a tracker and an ad frame. */
function loginPage() {
  const kids = [
    el('form', { action: 'http://insecure.test/login' }, [el('input', { type: 'password' })]),
    el('script', { src: 'https://cdn-a.test/x.js' }),
    el('script', { src: 'https://cdn-b.test/y.js' }),
    el('iframe', { src: 'https://ads.test/frame' }),
    el('img', {}),
    el('a', { href: 'https://out.test', target: '_blank' }),
  ];
  const flat = flatten(kids);
  const doc = { title: 'Login', querySelectorAll: (s) => matchAll(flat, s) };
  const loc = {
    href: 'http://shop.test/login?sid=SECRET',
    origin: 'http://shop.test',
    protocol: 'http:',
    pathname: '/login',
  };
  return perceive(doc, loc, Math.floor(Date.now() / 1000));
}

async function simulate(body) {
  const res = await fetch(`${BASE}/simulate`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json', Authorization: `Bearer ${TOKEN}` },
    body: JSON.stringify(body),
  });
  const json = await res.json();
  return { status: res.status, json };
}

test('a perceived page is accepted by the Rust deserialiser', { skip: !live && 'set SCEMA_OMNID_URL and SCEMA_OMNID_TOKEN' }, async () => {
  const { status, json } = await simulate({ world: loginPage(), goal: 'make this page safe' });
  assert.equal(status, 200, `daemon said: ${JSON.stringify(json)}`);
  assert.ok(json.record, 'expected a sealed record in the response');
});

test('the daemon marks a client-supplied world as client-supplied', { skip: !live && 'no daemon' }, async () => {
  // A record must never be able to claim a world that arrived over the wire was observed
  // locally. Enforced server-side, so a compromised extension cannot opt out.
  const { json } = await simulate({ world: loginPage(), goal: 'x' });
  assert.equal(json.record.world.observer, 'client:page');
});

test('simulate over HTTP persists nothing', { skip: !live && 'no daemon' }, async () => {
  const { json } = await simulate({ world: loginPage(), goal: 'x' });
  assert.equal(json.persisted, false);
  assert.equal(json.record_path, null);
});

test('the session token in the URL never reaches the record', { skip: !live && 'no daemon' }, async () => {
  const { json } = await simulate({ world: loginPage(), goal: 'x' });
  assert.ok(!JSON.stringify(json.record).includes('SECRET'), 'query strings must be stripped in perceive()');
});

test('an unreadable frame survives as a blind spot and raises uncertainty', { skip: !live && 'no daemon' }, async () => {
  const withFrame = await simulate({ world: loginPage(), goal: 'x' });
  assert.ok(
    withFrame.json.record.world.blind_spots.some((b) => /cross-origin/.test(b)),
    'the blind spot must cross the wire intact'
  );
  const u = withFrame.json.record.projections[0].uncertainty;
  assert.equal(u.measured, true);
  assert.ok(u.value > 0, 'a blind spot is measured evidence of ignorance');
});

test('an ungrounded goal loses to a grounded signal branch', { skip: !live && 'no daemon' }, async () => {
  // The runtime's central claim, checked across the wire: an instruction is not evidence.
  const { json } = await simulate({ world: loginPage(), goal: 'redesign the whole checkout' });
  const goalBranch = json.record.decision.ranked.find((r) => r.hypothesis === 'h-goal');
  assert.ok(goalBranch, 'the goal branch must still be ranked, not dropped');
  const proj = json.record.projections.find((p) => p.hypothesis === 'h-goal');
  assert.equal(proj.expected_gain.measured, false, 'no observed basis for a gain');
  assert.notEqual(json.record.decision.chosen, 'h-goal');
});

test('grounding the goal deliberately does change the outcome', { skip: !live && 'no daemon' }, async () => {
  const { json } = await simulate({
    world: loginPage(),
    goal: 'make this login page safe',
    ground: ['password-on-insecure-page'],
  });
  const proj = json.record.projections.find((p) => p.hypothesis === 'h-goal');
  assert.equal(proj.expected_gain.measured, true);
  assert.deepEqual(json.dangling_grounds, []);
});

test('a grounding id that names no signal is reported, not silently dropped', { skip: !live && 'no daemon' }, async () => {
  const { json } = await simulate({ world: loginPage(), goal: 'x', ground: ['typo:not-a-signal'] });
  assert.deepEqual(json.dangling_grounds, ['typo:not-a-signal']);
});

test('the reversibility term is unmeasured on an unknown domain', { skip: !live && 'no daemon' }, async () => {
  // A page world declares `domain: unknown`, so nothing knows what undoing an edit here
  // would cost. That has to arrive as unmeasured rather than as an optimistic default.
  const { json } = await simulate({ world: loginPage(), goal: 'x' });
  const proj = json.record.projections.find((p) => p.hypothesis === 'h-goal');
  assert.equal(proj.reversibility.measured, false);
  assert.equal(proj.reversibility.value, 0);
});
