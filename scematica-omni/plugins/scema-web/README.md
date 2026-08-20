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
# 3. right-click the extension → Options → paste the base URL and token → Save and test
# 4. open any page and click the toolbar button
```

```console
$ npm test          # 13 tests, hermetic and offline
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

### 1. It reads nothing until you click

There is no `content_scripts` block in the manifest and no `<all_urls>` host permission.
The only host permission is `http://127.0.0.1/*`. Perception happens through
`chrome.scripting.executeScript` on the toolbar click, under `activeTab` — one tab, one
interaction. An agent runtime that quietly read every page you visited would be the wrong
shape of thing to install, whatever it did with the data.

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
| `target-blank-without-noopener` | `target=_blank` links with no `noopener` in `rel` |
| `images-without-alt` | `<img>` with an absent or empty `alt` |

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
| `src/perceive.js` | pure DOM → `WorldState`. Runs in the browser and under `node --test`. |
| `src/content.js` | the HUD, in a closed shadow root. Holds no credentials. |
| `src/background.js` | service worker. Holds the token, maps message types to paths. |
| `src/options.html`, `src/options.js` | pairing, and a probe that tests the authenticated path. |
| `test/perceive.test.js` | perception, on a hand-built fake document. |
| `test/wire.test.js` | the JS↔Rust contract, against a live daemon. |

## Firefox

Untested. MV3 background service workers and `chrome.scripting` differ enough that it needs
its own pass; the perception module is portable as-is.
