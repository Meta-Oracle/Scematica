/**
 * Page perception: a DOM in, a `WorldState` out.
 *
 * This is `scema-tools`' `Observer` contract, in the browser. The output is the same JSON
 * `RepoObserver` produces, which is the whole point — `POST /simulate` cannot tell whether
 * a world came from a filesystem walk or a web page, so nothing above perception needed a
 * single line changed to gain a second sensory organ.
 *
 * The three obligations from `scema_tools::observer` apply here unchanged, and the browser
 * makes the third one vivid:
 *
 *   1. Report what could not be read. A cross-origin iframe is genuinely unreadable, and it
 *      goes in `blind_spots` rather than being silently skipped. This is not a limitation
 *      to apologise for — it is the single most useful thing this observer knows.
 *   2. Never round an unread thing to zero. An unreadable frame is not a frame with no
 *      forms in it.
 *   3. Say whether the walk was complete. `MAX_NODES` bounds the scan; hitting it produces
 *      `extent.total === null`, which `scema-sim` turns into measured uncertainty.
 *
 * ## Counts only
 *
 * Every signal here is a count of something in the document: forms posting off-origin,
 * images with no alt text, distinct third-party script origins. Nothing estimates a
 * probability or a severity. That is what lets `scema-sim` treat these as `measured` and
 * score a real expected gain from them; a "page quality score" invented here would be a
 * hallucination with a decimal point on it, laundered into a decision record.
 *
 * ## Domain is `unknown`, deliberately
 *
 * `Domain` exists so a specialist can decline. Guessing that a page on github.com is a
 * `software` world would be exactly that — a guess — and a wrong one whenever somebody is
 * reading a novel in a repository README. Nothing downstream needs it to be anything else.
 *
 * Pure and dependency-free so `test/perceive.test.js` can drive it with a hand-built fake
 * document under `node --test`, with no browser and no jsdom.
 */

/** Elements scanned before the walk reports an unbounded extent. */
const MAX_NODES = 5000;
/** Blind spots recorded before the list is truncated. */
const MAX_BLIND_SPOTS = 20;

/** Origin of a URL, or null when it will not parse. */
function originOf(url, base) {
  try {
    return new URL(url, base).origin;
  } catch {
    return null;
  }
}

function attr(el, name) {
  return (el.getAttribute && el.getAttribute(name)) || '';
}

function all(doc, selector) {
  const found = doc.querySelectorAll ? doc.querySelectorAll(selector) : [];
  return Array.prototype.slice.call(found);
}

function signal(id, polarity, label, detail, magnitude, targets, evidence) {
  return {
    id,
    polarity,
    label,
    detail,
    // Clamped, and always derived from a count. `measured: true` is a claim that somebody
    // counted this, and it is the claim `scema-sim` relies on to score a gain at all.
    magnitude: Math.max(0, Math.min(1, magnitude)),
    measured: true,
    targets,
    evidence,
  };
}

function object(id, kind, label, attrs) {
  return { id, kind, label, attrs, provenance: { kind: 'live', age_secs: 0 } };
}

/** Wrap a JS value as a `scema_world::Scalar`. */
function num(v) {
  return { t: 'int', v: Math.round(v) };
}
function bool(v) {
  return { t: 'bool', v: !!v };
}
function text(v) {
  return { t: 'text', v: String(v) };
}

/**
 * Perceive a document.
 *
 * @param {object} doc  something with `querySelectorAll` and `title`
 * @param {object} loc  something with `href`, `origin`, `protocol`
 * @param {number} now  unix seconds
 * @returns {object} a `scema_world::WorldState`
 */
function perceive(doc, loc, now) {
  const blindSpots = [];
  const objects = [];
  const signals = [];

  const secure = loc.protocol === 'https:' || loc.origin === 'null';
  const totalNodes = all(doc, '*').length;
  const truncated = totalNodes > MAX_NODES;

  // ── Forms ────────────────────────────────────────────────────────────────
  const forms = all(doc, 'form').slice(0, 200);
  let insecureForms = 0;
  let passwordForms = 0;
  forms.forEach((f, i) => {
    const action = attr(f, 'action');
    const actionOrigin = action ? originOf(action, loc.href) : loc.origin;
    const offOrigin = !!actionOrigin && actionOrigin !== loc.origin;
    const passwords = all(f, 'input[type=password]').length;
    const fields = all(f, 'input, select, textarea').length;
    const plaintextTarget = !!action && /^http:/i.test(action);

    if (plaintextTarget) insecureForms += 1;
    if (passwords > 0) passwordForms += 1;

    objects.push(
      object(`form:${i}`, 'form', attr(f, 'name') || attr(f, 'id') || `form ${i + 1}`, {
        fields: num(fields),
        password_fields: num(passwords),
        method: text((attr(f, 'method') || 'get').toLowerCase()),
        action_origin: text(actionOrigin || '(same document)'),
        off_origin: bool(offOrigin),
      })
    );
  });

  if (insecureForms > 0) {
    signals.push(
      signal(
        'form-posts-plaintext',
        'risk',
        `${insecureForms} form(s) submit over plain http`,
        'The form action is an http: URL, so the submitted values travel unencrypted.',
        insecureForms / 3,
        forms.map((_, i) => `form:${i}`).slice(0, 5),
        [`counted ${insecureForms} of ${forms.length} form(s) with an http: action`]
      )
    );
  }
  if (passwordForms > 0 && !secure) {
    signals.push(
      signal(
        'password-on-insecure-page',
        'risk',
        `${passwordForms} password field(s) on a page served over ${loc.protocol}`,
        'The page itself was not delivered over https, so the form and its script can be altered in transit.',
        1,
        [],
        [`counted ${passwordForms} form(s) containing a password input; page protocol is ${loc.protocol}`]
      )
    );
  }

  // ── Third-party script origins ───────────────────────────────────────────
  const scriptOrigins = new Map();
  const scriptsWithSrc = all(doc, 'script[src]');
  let offOriginScriptTags = 0;
  scriptsWithSrc.forEach((s) => {
    // `new URL(src, base)` resolves almost anything as a *relative* path rather than
    // throwing, so a nonsense src lands on this page's own origin and is correctly not
    // third-party. Only a src that cannot be resolved at all yields null.
    const o = originOf(attr(s, 'src'), loc.href);
    if (o && o !== loc.origin) {
      offOriginScriptTags += 1;
      scriptOrigins.set(o, (scriptOrigins.get(o) || 0) + 1);
    }
  });
  scriptOrigins.forEach((count, origin) => {
    objects.push(
      object(`script-origin:${origin}`, 'script-origin', origin, { scripts: num(count) })
    );
  });
  if (scriptOrigins.size > 0) {
    signals.push(
      signal(
        'third-party-scripts',
        'risk',
        `${scriptOrigins.size} third-party script origin(s)`,
        Array.from(scriptOrigins.keys()).slice(0, 5).join(', '),
        scriptOrigins.size / 10,
        Array.from(scriptOrigins.keys()).slice(0, 5).map((o) => `script-origin:${o}`),
        [
          `counted ${scriptOrigins.size} distinct third-party origin(s) across ` +
            `${offOriginScriptTags} off-origin script tag(s), of ${scriptsWithSrc.length} with a src`,
        ]
      )
    );
  }

  // ── Frames: the honest blind spot ────────────────────────────────────────
  const frames = all(doc, 'iframe');
  let unreadable = 0;
  frames.forEach((f) => {
    const src = attr(f, 'src');
    const o = src ? originOf(src, loc.href) : loc.origin;
    if (o && o !== loc.origin) {
      unreadable += 1;
      if (blindSpots.length < MAX_BLIND_SPOTS) {
        // Not "an empty frame". A frame whose contents the same-origin policy forbids us
        // from reading, which is a different statement and the one that is true.
        blindSpots.push(`iframe ${o} (cross-origin: contents unreadable from this document)`);
      }
    }
  });
  if (unreadable > 0) {
    objects.push(
      object('frames:cross-origin', 'frame-group', `${unreadable} cross-origin frame(s)`, {
        frames: num(unreadable),
      })
    );
  }

  // ── Accessibility: images without alt text ───────────────────────────────
  const images = all(doc, 'img');
  const noAlt = images.filter((img) => !attr(img, 'alt')).length;
  if (noAlt > 0) {
    signals.push(
      signal(
        'images-without-alt',
        'opportunity',
        `${noAlt} of ${images.length} image(s) have no alt text`,
        'Screen readers announce nothing for these.',
        noAlt / 20,
        [],
        [`counted ${noAlt} <img> element(s) with an empty or absent alt attribute`]
      )
    );
  }

  // ── The document itself ──────────────────────────────────────────────────
  const links = all(doc, 'a[href]');
  const offOriginLinks = links.filter((a) => {
    const o = originOf(attr(a, 'href'), loc.href);
    return o && o !== loc.origin;
  }).length;
  const unsafeTargets = links.filter(
    (a) => attr(a, 'target') === '_blank' && !/noopener/i.test(attr(a, 'rel'))
  ).length;

  objects.push(
    object('document', 'document', doc.title || loc.href, {
      elements: num(totalNodes),
      links: num(links.length),
      off_origin_links: num(offOriginLinks),
      forms: num(forms.length),
      images: num(images.length),
      frames: num(frames.length),
      https: bool(secure),
    })
  );

  if (unsafeTargets > 0) {
    signals.push(
      signal(
        'target-blank-without-noopener',
        'risk',
        `${unsafeTargets} link(s) open a new tab without rel=noopener`,
        'The opened page receives a handle to this one via window.opener.',
        unsafeTargets / 10,
        [],
        [`counted ${unsafeTargets} of ${links.length} link(s) with target=_blank and no noopener`]
      )
    );
  }

  return {
    // Rewritten to `client:page` by the daemon. Named honestly here anyway, so a world
    // dumped straight to a file is still self-describing.
    observer: 'page',
    entity: {
      kind: 'website',
      // Query and fragment are dropped: they routinely carry session tokens and search
      // terms, and this string is hashed into a decision record that outlives the tab.
      locator: `${loc.origin}${loc.pathname || ''}`,
      label: doc.title || loc.origin,
    },
    domain: 'unknown',
    observed_at: now,
    objects,
    facts: [],
    signals,
    extent: {
      observed: Math.min(totalNodes, MAX_NODES),
      // `null`, not the count. A capped scan does not know how much it missed, and saying
      // `total === observed` would claim completeness it cannot support.
      total: truncated ? null : totalNodes,
      note: truncated
        ? `scan capped at ${MAX_NODES} elements; the document is larger`
        : `scanned ${totalNodes} element(s)`,
    },
    blind_spots: blindSpots,
  };
}

const ScemaPerceive = { perceive, MAX_NODES, originOf };

// Content script: assign to the isolated world's global, which `content.js` reads.
if (typeof globalThis !== 'undefined') globalThis.ScemaPerceive = ScemaPerceive;
// Node test runner.
if (typeof module !== 'undefined' && module.exports) module.exports = ScemaPerceive;
