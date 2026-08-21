/**
 * The route table, tested without a browser.
 *
 * `background.js` is the boundary that makes "the caller names a tool, never a URL" true.
 * Everything a content script or the popup can reach is a key in [`ROUTES`], and the only
 * caller-supplied *segment* anywhere is a record id — which is the one crack in the rule and
 * therefore the thing most worth pinning.
 *
 * The file loads under Node because its listener registration is guarded. That guard is not
 * a testing hack: an unguarded `chrome.runtime.onMessage.addListener` at import time makes
 * the pure parts of a service worker untestable, which is how route tables end up unverified.
 */

const test = require('node:test');
const assert = require('node:assert');
const { ROUTES, normalizeBase, DEFAULT_BASE } = require('../src/background.js');

test('every route names a literal path or a builder, never a caller string', () => {
  for (const [key, r] of Object.entries(ROUTES)) {
    if (typeof r.path === 'string') {
      assert.ok(r.path.startsWith('/'), `${key}: ${r.path}`);
      assert.equal(r.param, undefined, `${key} has a literal path and a param`);
    } else {
      assert.equal(typeof r.path, 'function', `${key}: path is neither a string nor a builder`);
      assert.ok(r.param, `${key} builds a path but declares no param to validate`);
      assert.ok(r.param.pattern instanceof RegExp, `${key}: param.pattern must be a RegExp`);
    }
  }
});

test('only health skips the token', () => {
  // A route that forgot `auth` would be sent unauthenticated, get a 401, and look like a
  // pairing problem.
  for (const [key, r] of Object.entries(ROUTES)) {
    assert.equal(r.auth, key !== 'health', `${key}: auth is ${r.auth}`);
  }
});

test('every method is GET or POST', () => {
  for (const [key, r] of Object.entries(ROUTES)) {
    assert.ok(['GET', 'POST'].includes(r.method), `${key}: ${r.method}`);
  }
});

test('the record id pattern refuses everything that is not an id', () => {
  const { pattern } = ROUTES.record.param;
  // Real ids: eight hex characters, as `Commitment::root` is truncated to.
  assert.ok(pattern.test('8f92a1c4'));
  assert.ok(pattern.test('DEADBEEF'));

  // The shapes that would turn "pick a tool" back into "pick a URL".
  for (const bad of [
    '../../policy',
    '..%2f..%2fpolicy',
    'x/../../etc/passwd',
    'http://evil.test/',
    '8f92a1c4/verify',
    '8f92a1c4?x=1',
    '',
    'abc', // shorter than any real id
    'zzzzzzzz', // not hex
    'a'.repeat(65), // longer than any digest
  ]) {
    assert.ok(!pattern.test(bad), `pattern accepted ${JSON.stringify(bad)}`);
  }
});

test('the path builder percent-encodes what it is given', () => {
  // Belt and braces behind the pattern: even if the pattern were widened by accident, a
  // slash could not become a path separator.
  assert.equal(ROUTES.record.path('8f92a1c4'), '/decisions/8f92a1c4');
  assert.equal(ROUTES.record.path('a/b'), '/decisions/a%2Fb');
});

test('decide is in the table on purpose, and the daemon is what refuses it', () => {
  // Not an oversight. The daemon answers 403 without `--allow-decide`, so the authority
  // lives where it can be audited rather than in whether this table happens to list a
  // route. A client that could not even name the endpoint would make "why did nothing
  // happen" harder to answer, not safer.
  assert.ok(ROUTES.decide);
  assert.equal(ROUTES.decide.method, 'POST');
});

test('a base URL is normalised on read as well as on write', () => {
  // The `/mesh` pairing bug, in the extension. An old pairing in somebody's storage must
  // start working without a re-pair, so the strip runs every time the base is read.
  assert.equal(normalizeBase('http://127.0.0.1:7842/'), 'http://127.0.0.1:7842');
  assert.equal(normalizeBase('http://127.0.0.1:7842///'), 'http://127.0.0.1:7842');
  assert.equal(normalizeBase('  http://127.0.0.1:7842  '), 'http://127.0.0.1:7842');
});

test('a pasted endpoint URL is stripped back to the root', () => {
  // Somebody will paste the URL they just curled. Without this, `base + path` becomes
  // `/health/policy` and every request 404s.
  for (const suffix of ['health', 'policy', 'observe', 'simulate', 'decide', 'decisions']) {
    assert.equal(
      normalizeBase(`http://127.0.0.1:7842/${suffix}`),
      'http://127.0.0.1:7842',
      suffix
    );
  }
});

test('an empty base falls back to loopback rather than to a relative URL', () => {
  // `'' + '/health'` is a same-origin request from the service worker, which would be a
  // request to the extension itself and would fail in a way that reads as a broken daemon.
  assert.equal(normalizeBase(''), DEFAULT_BASE);
  assert.equal(normalizeBase(null), DEFAULT_BASE);
  assert.equal(normalizeBase(undefined), DEFAULT_BASE);
  assert.ok(DEFAULT_BASE.startsWith('http://127.0.0.1'));
});

test('the memory stats route is the one with a two-segment path', () => {
  // Pinned because a single-segment guess (`/memory`) 404s and the failure surfaces as an
  // empty panel rather than as an error.
  assert.equal(ROUTES.memory.path, '/memory/stats');
});
