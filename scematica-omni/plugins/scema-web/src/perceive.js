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
// The world-contract version this producer writes. Bump only alongside
// `scema_world::WORLD_SCHEMA`; `scema check` reports a mismatch in either direction.
const WORLD_SCHEMA = 'scema.world/1';

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

  // ── Subresource integrity on the third-party scripts ─────────────────────
  //
  // Counted separately from the origin count above, because they are different facts. A
  // page can load ten scripts from one CDN it has pinned with an `integrity` hash, and that
  // is a very different posture from one script from one CDN it has not.
  const offOriginNoSri = scriptsWithSrc.filter((sc) => {
    const o = originOf(attr(sc, 'src'), loc.href);
    return o && o !== loc.origin && !attr(sc, 'integrity');
  }).length;
  if (offOriginNoSri > 0) {
    signals.push(
      signal(
        'third-party-scripts-without-sri',
        'risk',
        `${offOriginNoSri} third-party script(s) load without an integrity hash`,
        'Whatever that origin serves next runs here, with this page’s privileges.',
        offOriginNoSri / 10,
        Array.from(scriptOrigins.keys()).slice(0, 5).map((o) => `script-origin:${o}`),
        [`counted ${offOriginNoSri} of ${offOriginScriptTags} off-origin script tag(s) with no integrity attribute`]
      )
    );
  }

  // ── Mixed content ────────────────────────────────────────────────────────
  //
  // Only meaningful on an https page: an http subresource on an http page is not "mixed",
  // it is consistent, and the page-level `password-on-insecure-page` signal already covers
  // the real problem there. Reporting it twice would double-count into the same decision.
  let mixed = 0;
  if (secure) {
    const subresources = [
      ...all(doc, 'script[src]'),
      ...all(doc, 'img[src]'),
      ...all(doc, 'iframe[src]'),
      ...all(doc, 'audio[src]'),
      ...all(doc, 'video[src]'),
      ...all(doc, 'link[href]'),
    ];
    mixed = subresources.filter((n) => {
      const raw = attr(n, 'src') || attr(n, 'href');
      return /^http:\/\//i.test(raw);
    }).length;
  }
  if (mixed > 0) {
    signals.push(
      signal(
        'mixed-content-subresources',
        'risk',
        `${mixed} subresource(s) load over plain http on an https page`,
        'The lock in the address bar does not cover these; they can be replaced in transit.',
        mixed / 5,
        [],
        [`counted ${mixed} element(s) whose src or href begins with http://, on a page served over ${loc.protocol}`]
      )
    );
  }

  // ── Inline event handlers ────────────────────────────────────────────────
  //
  // Counted rather than judged. A page full of `onclick=` is not necessarily insecure; it
  // is a page whose behaviour cannot be governed by a script-src CSP, which is a fact worth
  // having and is not the same claim.
  const INLINE_HANDLER_SELECTOR =
    '[onclick],[onload],[onerror],[onmouseover],[onmouseout],[onsubmit],[onfocus],[onblur],[onchange],[oninput],[onkeydown],[onkeyup]';
  const inlineHandlers = all(doc, INLINE_HANDLER_SELECTOR).length;
  if (inlineHandlers > 0) {
    signals.push(
      signal(
        'inline-event-handlers',
        'risk',
        `${inlineHandlers} element(s) carry an inline event handler`,
        'Inline handlers run outside any script-src policy the page sets.',
        inlineHandlers / 40,
        [],
        [`counted ${inlineHandlers} element(s) matching ${INLINE_HANDLER_SELECTOR.split(',').length} on* attribute selectors`]
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

  // ── Form fields with nothing to announce ─────────────────────────────────
  //
  // A control is considered labelled if it carries `aria-label`, `aria-labelledby` or
  // `title`, or if some `<label for=...>` names its id. Wrapping labels (`<label><input>`)
  // are not detected and are therefore counted as unlabelled, which makes this an
  // *over*-count. That direction is chosen deliberately: an over-count produces a branch
  // somebody investigates and dismisses, an under-count produces silence, and the evidence
  // string says which way it errs so a reader is not misled by the number.
  const labelledIds = new Set(
    all(doc, 'label[for]')
      .map((l) => attr(l, 'for'))
      .filter(Boolean)
  );
  const controls = all(doc, 'input, select, textarea').filter(
    (c) => !['hidden', 'submit', 'button', 'reset', 'image'].includes((attr(c, 'type') || '').toLowerCase())
  );
  const unlabelled = controls.filter(
    (c) =>
      !attr(c, 'aria-label') &&
      !attr(c, 'aria-labelledby') &&
      !attr(c, 'title') &&
      !(attr(c, 'id') && labelledIds.has(attr(c, 'id')))
  ).length;
  if (unlabelled > 0) {
    signals.push(
      signal(
        'controls-without-labels',
        'opportunity',
        `${unlabelled} of ${controls.length} form control(s) have no accessible name`,
        'A screen reader announces the control type and nothing else.',
        unlabelled / 15,
        [],
        [
          `counted ${unlabelled} control(s) with no aria-label, aria-labelledby, title, or matching <label for>; ` +
            'wrapping labels are not detected, so this over-counts rather than under-counts',
        ]
      )
    );
  }

  // ── Heading levels ───────────────────────────────────────────────────────
  const headings = all(doc, 'h1, h2, h3, h4, h5, h6');
  let skips = 0;
  let previous = 0;
  headings.forEach((h) => {
    const level = Number(h.tag ? h.tag.slice(1) : (h.tagName || 'h1').slice(1));
    if (previous && level > previous + 1) skips += 1;
    previous = level;
  });
  if (skips > 0) {
    signals.push(
      signal(
        'heading-level-skips',
        'opportunity',
        `${skips} place(s) where the heading level jumps by more than one`,
        'Assistive technology uses heading level as document structure, so a skip reads as a missing section.',
        skips / 6,
        [],
        [`counted ${skips} skip(s) across ${headings.length} heading(s) in document order`]
      )
    );
  }

  // ── javascript: links ────────────────────────────────────────────────────
  const jsLinks = all(doc, 'a[href]').filter((a) => /^javascript:/i.test(attr(a, 'href'))).length;
  if (jsLinks > 0) {
    signals.push(
      signal(
        'javascript-url-links',
        'risk',
        `${jsLinks} link(s) navigate to a javascript: URL`,
        'The href is executed as script, so it is a code path a CSP cannot see and a reader cannot preview.',
        jsLinks / 10,
        [],
        [`counted ${jsLinks} <a href> value(s) beginning with javascript:`]
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
      headings: num(headings.length),
      controls: num(controls.length),
      inline_handlers: num(inlineHandlers),
      mixed_content: num(mixed),
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
    // The contract version. A producer that does not declare one is refused by the
    // importer, because an undeclared version is what makes the next change to the format
    // a silent misread instead of an error message.
    schema: WORLD_SCHEMA,
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
    // `web`, not `unknown`. Before the domain vocabulary opened, a perceived page and a
    // set of oracle feeds both had to report `unknown`, which made two entirely different
    // worlds indistinguishable to every specialist downstream. Naming it does not make any
    // specialist claim competence here — they match the arm they understand and decline on
    // everything else — it just stops the decline from being uninformative.
    domain: 'web',
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
