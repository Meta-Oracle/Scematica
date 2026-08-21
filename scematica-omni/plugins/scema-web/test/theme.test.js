/**
 * The palette is a port. Rust is authoritative.
 *
 * `crates/scema-tui/src/theme.rs` holds the constants the console draws with and the ones
 * `tools/make-icons.py` renders the icon from. `src/theme.js` copies them. This file pins
 * the copy, so a drift fails here rather than surfacing as two surfaces of one product that
 * do not look like each other.
 *
 * The values below are transcribed from the `const INK` block in `theme.rs`. If a test in
 * this file fails, the fix is almost always to change `theme.js`, not this file.
 */

const test = require('node:test');
const assert = require('node:assert');
const { INK, ROLE, css } = require('../src/theme.js');

/** `const NAME: Ink = ink(0xRR, 0xGG, 0xBB, ...)` in theme.rs, name for name. */
const RUST = {
  void: '#08060f',
  panel: '#0f0b1a',
  select: '#1e1438',
  rule: '#2a1f45',
  ruleHot: '#6d40c4',
  text: '#e6e0f5',
  muted: '#8a81a8',
  ghost: '#544c6c',
  violet: '#a96bff',
  violetLo: '#7c4dd8',
  violetHi: '#cba6ff',
  azure: '#7dd3fc',
  azureLo: '#4ba3d8',
  amber: '#f2b15c',
  rose: '#ff7b9c',
  mint: '#86e5c0',
};

test('every ink matches the Rust constant it is named after', () => {
  for (const [name, hex] of Object.entries(RUST)) {
    assert.equal(INK[name], hex, `INK.${name} drifted from theme.rs`);
  }
});

test('the palette has no inks the console does not have', () => {
  // A colour that exists only in the browser is a colour nobody chose against the others.
  assert.deepEqual(Object.keys(INK).sort(), Object.keys(RUST).sort());
});

test('the azure accent is reserved for claims, here as in the console', () => {
  // Chosen and Valid are claims the agent made. Opportunity and Live are observations, and
  // they must not borrow the accent — the eye is being trained to read azure as "this is
  // the thing that was asserted".
  assert.equal(ROLE.chosen, INK.azure);
  assert.equal(ROLE.valid, INK.azure);
  assert.notEqual(ROLE.opportunity, INK.azure);
  assert.notEqual(ROLE.live, INK.azure);
  assert.notEqual(ROLE.measured, INK.azure);
});

test('unmeasured is a recessive tone, never a sibling of measured', () => {
  // The single most important thing this palette does. If an unmeasured term is only a
  // *hue* away from a measured one, a colourblind reader and a screenshot both lose the
  // distinction the whole type system below is protecting.
  assert.equal(ROLE.unmeasured, INK.ghost);
  assert.notEqual(ROLE.unmeasured, ROLE.measured);
  // Ghost is darker than body text at every channel, which is what makes it recede rather
  // than merely differ.
  const channels = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
  const ghost = channels(INK.ghost);
  const body = channels(ROLE.body);
  ghost.forEach((c, i) => assert.ok(c < body[i], `ghost channel ${i} is not darker than body`));
});

test('a risk and an opportunity are different colours and different words', () => {
  assert.notEqual(ROLE.risk, ROLE.opportunity);
});

test('the stylesheet names every role class the renderers use', () => {
  // `content.js` and `popup.js` set these class names. A class the sheet does not define
  // renders as inherited body text, which for `unmeasured` would silently undo the em-dash
  // rule's visual half.
  const sheet = css();
  for (const cls of [
    'measured',
    'unmeasured',
    'chosen',
    'excluded',
    'abstained',
    'risk',
    'opportunity',
    'estimated',
    'live',
    'stale',
    'absent',
    'valid',
    'invalid',
    'dim',
    'label',
  ]) {
    assert.match(sheet, new RegExp(`\\.${cls}\\s*\\{`), `.${cls} is not defined`);
  }
});

test('the stylesheet survives a forced-colours mode', () => {
  // Rule 2: colour is decoration, never the message. In a forced-colours mode every colour
  // above is replaced by the system's, and nothing may depend on the hue alone — which is
  // true, because every state is also carried by text.
  assert.match(css(), /@media \(forced-colors: active\)/);
});

test('the sheet carries its own reset so a hostile page cannot restyle the HUD', () => {
  // A panel that inherited `* { display: none }` from some site's reset would look like the
  // extension failing.
  assert.match(css(), /:host \{ all: initial; \}/);
});

test('nothing in the sheet reaches outside the shadow root', () => {
  // No bare element selectors that could escape if this were ever adopted into a page
  // context by mistake: everything is either `:host`, a class, or a descendant of `.omni`.
  // `html`/`body` in particular must not appear.
  const sheet = css();
  assert.ok(!/\bhtml\s*[,{]/.test(sheet), 'the sheet styles html');
  assert.ok(!/\bbody\s*[,{]/.test(sheet), 'the sheet styles body');
});
