# Scema Omni — browser extension (MV3)

The browser as the agent's second sensory organ. A content script perceives the current
page as a `WorldState`, the daemon runs the same loop it runs over a source tree, and the
HUD shows the matrix — including what could not be seen.

No build step, no bundler, no dependencies. Load it unpacked and it runs.

```console
# 1. start the daemon
$ cd /path/to/a/project
$ scema-omnid --allow .
  token       .scema/omnid.token

# 2. chrome://extensions → Developer mode → Load unpacked → this directory
# 3. click the toolbar icon → Pairing… → paste the base URL and token → Save and test
# 4. open any page and press "Perceive this page" (or Alt+Shift+O)
```

```console
$ npm test          # 44 tests, hermetic and offline
$ SCEMA_OMNID_URL=http://127.0.0.1:7846 \
  SCEMA_OMNID_TOKEN=$(cat /tmp/w/.scema/omnid.token) npm test    # + 9 wire tests
```

## The point: perception is the only new part

`src/perceive.js` emits exactly the JSON `scema_tools::RepoObserver` emits. Nothing above
perception changed to gain a browser — `POST /simulate` cannot tell a DOM from a filesystem
walk, because both are a `WorldState`. That is what the two-dependency rule on
`scema-world` bought.

Verified rather than asserted: `test/wire.test.js` posts a real perceived page to a real
daemon and checks the decision that comes back, so an enum tag representation drifting
between the JS and the Rust fails a test instead of surfacing as a 400 weeks later.

## Four design decisions

### 1. It reads nothing until you ask

There is no `content_scripts` block in the manifest and no `<all_urls>` host permission.
The only host permission is `http://127.0.0.1/*`. Perception happens through
`chrome.scripting.executeScript` on the popup's button or the keyboard command, under
`activeTab` — one tab, one interaction. An agent runtime that quietly read every page you
visited would be the wrong shape of thing to install, whatever it did with the data.

The popup replaced a bare toolbar click, which turned one click into two. What it buys is
that the extension can now **say something before it reads anything**. Before, a broken
pairing surfaced as an overlay that appeared on somebody's banking page, said
`unreachable`, and left them guessing whether the daemon was down, the token was wrong or
the URL was. The keyboard command (`Alt+Shift+O`) is the one-gesture path back: a command
invocation grants `activeTab` exactly as a toolbar click does, so it is the same permission
with one fewer step rather than a way around one.

### 2. The token never reaches the page

A content script runs in an isolated world, but it runs *on the page*. If it held the
daemon token, one subverted overlay would let a hostile page drive the operator's agent
over loopback for as long as the pairing lasted. So:

```
  content script  →  "here is a world, please simulate it"   (no credentials)
  service worker  →  adds Authorization, fetches             (holds the secret)
```

The content script cannot name a URL either. It sends a message *type*, and
`background.js` maps the type to a path — the same rule as `lib/scylar/tools.ts` in the web
app, where the model picks a tool name and never an endpoint.

This is also why the service worker does the fetching rather than the content script: a
content-script `fetch` is subject to CORS and the daemon deliberately sends no
`Access-Control-Allow-Origin`. A service-worker fetch under `host_permissions` is not. That
one asymmetry is what lets the daemon refuse CORS outright and still be usable.

### 3. Blind spots are the most useful thing this observer knows

A cross-origin iframe is genuinely unreadable — the same-origin policy says so — and it
goes into `blind_spots` rather than being skipped. It then becomes *measured* uncertainty
in `scema-sim`, which is the correct treatment: the agent is less sure about this page
precisely because part of it is invisible, and it can say so with a number.

Reporting an unreadable frame as "no forms found" would be the browser version of rendering
an unreadable vault balance as zero.

### 4. The render rule travels with the data

`cell()` in `content.js` is the same function as `render::cell` in `scema-cli`: an
unmeasured term prints `—`, never `0.00`. A column of numbers is the most persuasive thing
a program can put on a screen, and the moment a neutral element renders as a number, the
distinction the whole Rust type system has been protecting is gone in the last hundred
lines of the product.

## What it counts

Counts only — nothing here estimates a probability or a severity, which is what lets
`scema-sim` treat these as measured and score a real expected gain from them.

| Signal | Counted from |
|---|---|
| `password-on-insecure-page` | password inputs, against `location.protocol` |
| `form-posts-plaintext` | form actions with an `http:` scheme |
| `third-party-scripts` | distinct off-origin `script[src]` origins |
| `third-party-scripts-without-sri` | off-origin `script[src]` with no `integrity` |
| `mixed-content-subresources` | `http:` subresources, **only** on an `https:` page |
| `inline-event-handlers` | elements carrying an `on*` attribute |
| `javascript-url-links` | `<a href>` beginning `javascript:` |
| `target-blank-without-noopener` | `target=_blank` links with no `noopener` in `rel` |
| `images-without-alt` | `<img>` with an absent or empty `alt` |
| `controls-without-labels` | form controls with no accessible name |
| `heading-level-skips` | `h1..h6` in document order, jumping by more than one |

Two of those carry a caveat in their own evidence string, because a number whose bias is
undocumented is a number a reader cannot calibrate against:

* `mixed-content-subresources` is counted **only** on a page that is itself `https:`. An
  `http:` subresource on an `http:` page is not "mixed", it is consistent, and
  `password-on-insecure-page` already covers the real problem there. Counting it twice
  would double it into the same decision.
* `controls-without-labels` does not detect a wrapping `<label><input></label>`, so it
  **over-counts**. That direction is deliberate: an over-count produces a branch somebody
  investigates and dismisses, an under-count produces silence.

`domain` is always `unknown`. Guessing that a page on a code host is a `software` world
would be a guess, and a wrong one whenever somebody is reading a README as prose. Nothing
downstream needs it to be anything else — `Domain` exists so a specialist can decline.

The entity locator drops the query string and fragment. It is hashed into a decision record
that outlives the tab, and query strings routinely carry session tokens; `test/wire.test.js`
pins that a `?sid=SECRET` never reaches the record.

## Files

| File | Role |
|---|---|
| `manifest.json` | MV3. No content scripts, one host permission. |
| `src/theme.js` | the palette, ported from `crates/scema-tui/src/theme.rs`. One sheet, three surfaces. |
| `src/perceive.js` | pure DOM → `WorldState`. Runs in the browser and under `node --test`. |
| `src/content.js` | the HUD, in a closed shadow root. Holds no credentials. |
| `src/popup.html`, `src/popup.js` | pairing state, the perceive button, the record log. |
| `src/background.js` | service worker. Holds the token, maps message types to paths. |
| `src/options.html`, `src/options.js` | pairing, and a probe that tests the authenticated path. |
| `icons/`, `tools/make-icons.py` | the mark, generated from the same palette with stdlib only. |
| `test/perceive.test.js` | perception, on a hand-built fake document. |
| `test/theme.test.js` | the palette against the Rust constants it is a port of. |
| `test/routes.test.js` | the route table, including the record-id pattern. |
| `test/wire.test.js` | the JS↔Rust contract, against a live daemon. |

## Four more decisions, from the 0.2.0 pass

### The palette is a port, and Rust is authoritative

Before `src/theme.js` existed, the HUD was `#4c4cff`, the options page was `#8b8bff`, and
neither was the console's violet. `test/theme.test.js` transcribes the `const INK` block
from `crates/scema-tui/src/theme.rs` and fails on a drift. Black and violet, with soft blue
reserved for a claim — the same identity the terminal console has, so an operator with three
Scematica surfaces open can tell at a glance which one is making a claim about their money
and which one about a decision.

### The seal confirmation is not `window.confirm`

A content script shares the page's window, so `confirm` is whatever the page last assigned
to it. `window.confirm = () => true` and the dialog the operator never saw returns yes. The
confirmation therefore lives inside the closed shadow root, where the page cannot reach it.

The daemon still refuses `/decide` without `--allow-decide`, so this is defence in depth. It
is worth having anyway: the operator who *did* enable it is exactly the one whose
confirmation now matters.

### The one parameterised route is nailed shut

`GET /decisions/{id}` needs a caller-supplied segment, which is exactly the shape of request
that turns "pick a tool" back into "pick a URL". The route declares a pattern, the id is
matched against it *before* the path is built, and it is percent-encoded afterwards.
`test/routes.test.js` pins that `../../policy`, a full URL and a trailing `/verify` are all
refused here rather than sent and 404ing.

### It verifies nothing itself, on purpose

The commitment status shown for a sealed record is **the daemon's**, and it is labelled as
such on screen. Recomputing it here would mean a fourth implementation of the canonical
encoding — after `canonical.rs` and `web/lib/omni/canonical.ts` — and this project's rule is
that a copy which drifts is worse than no copy. One differing byte and the overlay reports
an untampered record as INVALID, which is the most damaging failure available: it teaches
the reader to stop believing the verifier.

So there is an **export** button instead. The JSON is written with two spaces, matching
`RecordStore::save`, so an exported file is byte-identical to the one on disk and either can
be handed to `scema verify --file` or dropped on the `/omni` page, which hashes it in the
reader's own browser and talks to nothing.

## Firefox

Untested. MV3 background service workers and `chrome.scripting` differ enough that it needs
its own pass; the perception module is portable as-is.
