# Scematica Omni

An agent runtime. Not a chatbot with tools bolted on — a loop that perceives an
environment, proposes competing futures, projects each one forward, ranks them under a
stated preference, decides *or refuses to*, and seals what it did into a record somebody
else can check.

```
  observe ─▶ hypothesise ─▶ simulate ─▶ score ─▶ decide ─▶ record ─▶ remember
     ▲                                                                  │
     └──────────────────────────────────────────────────────────────────┘
```

Four surfaces over one loop:

```console
$ cargo install scema-cli scema-daemon scema-mcp

$ scema simulate "clear the marker backlog" --ground markers:scema-tools   # CLI
$ scema-omnid --allow .                                                     # daemon
$ scema-mcp --allow .                                                       # MCP, for models
#   plugins/scema-web  → browser extension, the page as a world
#   /omni in web/      → verify a sealed record in a browser, offline
```

Embedding the loop rather than running it:

```toml
[dependencies]
scema-agent  = "0.1"   # the whole loop
scema-world  = "0.1"   # just the types, if you only need the wire format
```

## The one idea

Every layer here can say **"I don't know"**, and saying it costs nothing.

That sounds like a small design note. It is the whole product. An agent that cannot
express ignorance has to express something else instead, and what it produces is a number
of the right shape in the right column that nothing downstream can distinguish from a
measurement. Three mechanisms enforce it:

| Mechanism | Where | What it prevents |
|---|---|---|
| `Provenance` | `scema-world` | an unreadable thing rendering as `0` — "we could not see this" becoming "this is empty" |
| `Term { value, measured, note }` | everywhere | a score standing on nothing, silently |
| `Applicability` | `scema-policy` | a specialist answering a question outside its domain |

Every aggregate ships a **measured fraction** beside it, and no renderer may print the
score without it. A utility of `0.91` computed on two terms out of nine is a statement
about ignorance, and it has to look like one.

Concretely, on a real run against this workspace:

```
  #   BRANCH                                          GAIN    RISK    COST  UNCERT  REVERS   UTILITY  MEASURED
  ────────────────────────────────────────────────────────────────────────────────────────────────────────────
▸  1 take: 11 marker(s) in `scema-tools`             0.22    0.40    0.10    0.00    0.70     0.125       5/5
   2 add tests to the scema-cli crate                   —    0.40    0.10    0.00    0.70    -0.095       4/5
```

Branch 2 is what the operator typed. It scores negative and prints `—` for its expected
gain, because nothing observed supports it. **An instruction is not evidence.** That is not
the runtime being unhelpful; it is the runtime being right, and it is the behaviour every
other part of the design exists to protect.

## Crates

| Crate | Role |
|---|---|
| `scema-world` | the universal state representation — `WorldState`, `Goal`, `Hypothesis`, `Term`, `Provenance`. Pure types, no I/O, `serde` only. Also the wire format for the browser extension. |
| `scema-tools` | perception. `Observer` + `RepoObserver`. The only crate in the read path allowed to touch the outside world. |
| `scema-memory` | four memories — episodic, semantic, procedural, **counterfactual** — over append-only JSONL. |
| `scema-sim` | counterfactual projection. Refuses to invent a number. |
| `scema-policy` | `U = R − λ₁K − λ₂C − λ₃U + λ₄V`, plus pluggable evaluators that may decline. |
| `scema-verify` | canonical hashing and proof-carrying decision records. |
| `scema-agent` | the orchestration loop. |
| `scema-cli` | the `scema` binary. |
| `scema-daemon` | `scema-omnid` — a loopback-only, token-authenticated HTTP surface, on `std`. |
| `scema-mcp` | the loop as MCP tools, over stdio, with workspace-confined paths. |

| Elsewhere | Role |
|---|---|
| `plugins/scema-web` | MV3 browser extension. Perceives a page as a `WorldState`. No build step. |
| `../web/app/omni` | a decision-record verifier that runs entirely in the reader's browser. |

## Six decisions worth knowing about

### 1. The utility equation is additive, and that is a safety property

`U = R − λ₁K − λ₂C − λ₃U + λ₄V`. A multiplicative form is more expressive and it is a
trap: an unmeasured factor is either `1.0` (and the equation lies about certainty) or `0.0`
(and the score is pinned shut by dimensions nobody has built). This repository has paid for
the `0.0` version twice — once when an unmeasured perception channel jammed the sentience Ψ
at zero permanently, once when a literal reading of the agentic spec pinned a gate shut on
subsystems that did not exist. Additive terms with a `0.0` neutral make **ignorance silent
rather than fatal, and visible in the coverage** rather than smuggled into the number.

The λ weights are a *stated preference*, not a fitted parameter, and they are hashed into
every record so a ranking can be re-read later against the preferences that produced it.

### 2. The Deep Q* agent is a specialist, not the agent

`scematica-nn` is a trained Dueling Double-DQN with a real edge on Raydium pools. It is
wired in at `scema-policy::dqstar` as **one evaluator among several**, and on a software
world it declines:

```
EVALUATORS
  dqstar     OUT-OF-DOMAIN  world domain is Software; the 24-feature state is pool and
                            position data and has no reading of this
```

Its 24 features are pool age, liquidity, buy/sell ratio, position PnL. Asked to rank a
refactor it would still emit five finite Q-values, correctly shaped and entirely
meaningless. `Applicability` distinguishes **out-of-domain** (permanent, fine) from
**insufficient** (my domain, missing inputs — a file you could go and supply), because
collapsing those loses the operator's next action.

Specialist opinions are *attached*, never averaged into the ranking: a general utility and
a normalised Q-value are not the same quantity. A qualified specialist's measured negative
**vetoes** the top branch outright. A veto is legible; a weighted blend of incommensurable
scores is not.

### 3. Counterfactual memory is mostly unanswerable, on purpose

Every branch the agent declines is recorded with what was projected for it. Its *realised*
outcome almost never exists, because nobody ran it. So:

> **Unresolved counterfactuals are counted, never scored.**

```
CALIBRATION
  branches not taken, recorded   1
  of those, later resolved       0
  unresolved                     1
  mean |projected − realised|    — (nothing resolved; a branch nobody ran has no outcome)
```

Imputing outcomes for untaken branches — from a model, a neighbour, a prior — would mean
the system generating its own training signal, and every subsequent decision tuning itself
to a fiction. Same asymmetry the bot's own `calibration.rs` lives with.

### 4. Floats are hashed as fixed-point, because JSON transport is not bit-exact

Found the hard way. A record sealed in the daemon and verified after a `GET` reported
`INVALID` on a byte nobody had touched:

```text
  in memory   0.40066666666666667
  serialised  "0.40066666666666667"     <- identical text
  parsed back 0.4006666666666666        <- one ULP low
```

`serde_json`'s formatter is exact; its *parser* is not correctly rounded for every
17-significant-digit input. A verifier that cries tamper on an honest round trip is worse
than no verifier, because the first thing anyone does with one is stop believing it.

So floats are hashed as `round(v * 1e9)` in `i64`, and the commitment binds values **to
1e-9**. An edit at or above that resolution is caught; one below it is not, and cannot move
any gate in `scema-policy`. Both halves of that deal are pinned by tests on each side.

Same wall `scema-bot-mesh` hit, same conclusion: bit-exact float agreement between two
processes is engineered, not achieved by care. That crate reached for fixed-point
throughout; here only the hashing boundary needs it.

### 5. What a decision record proves, and what it does not

`scema verify` recomputes six SHA-256 digests (world, goal, hypotheses, projections,
policy, decision) and a root binding them with their field names.

```console
$ scema verify 234e11a0
234e11a0  INVALID
    projections  committed 93a8fc3e1f51…  recomputed c723b4db7b08…
    root         committed 234e11a0cd3c…  recomputed 83df34f1d80f…
```

* **It proves** the record was not edited after sealing, and names the field that moved.
* **It does not prove** the world was really like that. An observer that misread a
  repository produces a perfectly verifiable record of a wrong observation — *provenance*
  carries that, which is why the world state is committed whole, `Absent` arms and blind
  spots included.
* **It does not prove** the record is the original. Both can be regenerated by whoever
  holds the file. This is tamper-**evident** to a third party holding an earlier copy, not
  tamper-proof, until the root is anchored somewhere the author does not control.

The encoding is stricter than JSON — sorted keys, tagged types, normalised `-0.0` and NaN —
because `serde_json`'s output is not stable enough to hash. SHA-256 rather than the
keccak-256 in `scema-bot-mesh`, because nothing on an EVM verifies these yet; if one ever
does, that binding belongs on the keccak path in `mesh-core`.

### 6. Nothing here writes to the environment it observes

`scema execute`, `delegate`, `discover` and `pay` are registered verbs that exit non-zero
and say what is missing. They are in `--help` on purpose — the shape of the runtime
includes an action path, an agent-to-agent path and a payment path, and finding out from
the tool beats finding out later. A verb that silently did not exist would be
indistinguishable from one that failed.

The action path needs the approval model `alchem-link` already worked out (risk declared
per tool, no terminal means deny, secrets refused before the prompt). `pay` needs a spend
policy first: a runtime that can spend without one is a runtime nobody should install.

## The four surfaces

All four drive the same `scema-agent`. None of them re-implements perception, simulation or
verification, and none can reach a capability the others cannot — which is why the safety
argument only has to be made once.

### `scema` — the CLI

```console
$ scema observe .
$ scema simulate "clear the marker backlog" --ground markers:scema-tools
$ scema decide   "clear the marker backlog" --ground markers:scema-tools
$ scema explain 58898030 ; scema verify --all
$ scema policy
```

`simulate` never persists. `decide` seals a record and appends memory. `execute`,
`delegate`, `discover` and `pay` are registered and exit non-zero saying what is missing.

### `scema-omnid` — the local daemon

```console
$ scema-omnid --allow /path/to/a/project
  listening   http://127.0.0.1:7842   (loopback only, not configurable)
  token       .scema/omnid.token
```

Hand-rolled HTTP/1.1 on `std` — no `hyper`, no `rustls`, no async runtime. Partly for
consistency with the rest of the tree, mostly because the moment omni carries a TLS stack
somebody will path-depend it from the bot workspace and rediscover the
`zeroize`/`curve25519-dalek` conflict the root `Cargo.toml` documents at length.

Four guards, in order:

| Guard | Answers | Failure |
|---|---|---|
| loopback bind, **not configurable** | nothing off-machine connects | — |
| `Host` header check | DNS rebinding | `421` |
| 256-bit bearer token, constant-time | every local process and web page can reach `127.0.0.1` | `401` |
| `Workspace` resolve-then-compare | a caller naming `~/.ssh` | `403` |

No `Access-Control-Allow-Origin` is ever emitted and no `OPTIONS` is handled, so a web page
cannot read a reply even if it guesses a route. The extension is unaffected because it
fetches from its service worker under `host_permissions`, which is not subject to CORS —
that one asymmetry is what lets the daemon refuse CORS outright and still be usable.

`POST /decide` is off until `--allow-decide`. `POST /simulate` builds its own
non-persisting agent rather than flipping a flag on the shared one, because a shared mutable
flag is a race whose failure mode is a simulation quietly sealing a record.

### `plugins/scema-web` — the browser extension

MV3, no build step, no dependencies. A content script perceives the page as a `WorldState`
and the daemon runs the same loop it runs over a source tree — **perception is the only new
part.** `POST /simulate` cannot tell a DOM from a filesystem walk.

It reads nothing until you click: no `content_scripts` block, no `<all_urls>`, injection via
`activeTab` on the toolbar button. The token lives only in the service worker, never in the
content script, and the content script picks a message *type* rather than a URL.

The most useful thing it reports is what it could not see. A cross-origin iframe is
genuinely unreadable, so it becomes a blind spot and then *measured* uncertainty — the agent
is less sure about that page, and can say so with a number.

### `scema-mcp` — the loop as tools

```jsonc
{ "mcpServers": { "scema-omni": { "command": "scema-mcp", "args": ["--allow", "/proj"] } } }
```

Links the loop directly rather than proxying the daemon: same library, one less hop, no way
for the two surfaces to disagree about what the loop does.

Two guards specific to a model caller. Paths resolve through `Workspace` — not paranoia
about a hostile model, but because a *cooperative* one asked to audit a project will reason
its way to `~/.ssh`, since that is genuinely relevant to an audit. And `omni_decide` is not
advertised at all unless `--allow-decide`, because a tool that is listed and always fails
teaches a model to retry it.

The `initialize` response tells the model the two things it will otherwise get wrong: an em
dash is not a zero, and grounding is never inferred.

### `/omni` — the record console

The seventh product in `web/`, and the only one with no server side. Drop a record in; it is
read and hashed with WebCrypto in your own browser. No `/api/omni` route exists — a verifier
that had to send the record somewhere would be asking the reader to trust a third party in
order to avoid trusting one.

`lib/omni/canonical.ts` is a **port** of `crates/scema-verify/src/canonical.rs` and Rust is
authoritative. `npm run check:omni` re-derives the digests of a real `scema decide` record
and compares them to the ones Rust wrote into it, so drift fails a check rather than
surfacing as an untampered record reported `INVALID`.

One trap it pins: the page verifies the **raw text**, never a re-serialised object.
`JSON.parse` collapses Rust's `0.0` to `0` and `JSON.stringify` writes it back without the
fraction, moving it from the float tag to the integer tag and changing the digest. Nothing
would be wrong with the record — the round trip destroys information the encoding depends
on.

## Why its own cargo workspace

The third tree in this repository to make that call, after `scema-botchain` and
`scema-bot-mesh`, and for the same lockfile reason spelled out in the root `Cargo.toml`:
that workspace is pinned around `solana-sdk 1.18` (reqwest 0.11, curve25519-dalek 3,
zeroize < 1.4), and omni's surfaces want a modern HTTP stack.

There is a second reason specific to this tree. **Omni is domain-agnostic by design**: a
repository, a web page and a market are all just `WorldState`. A dependency on the bot's
core would make the trading domain structurally privileged, and the first thing anyone
would do is reach for it. So nothing here may depend on `scematica-core` or anything
downstream of it. It reaches back only into the crates the root `CLAUDE.md` lists as
solana-free — today, `scematica-nn` with `default-features = false`.

## Build and test

```powershell
cd scematica-omni
cargo build --release
cargo test --workspace                 # 159 tests
cargo clippy --workspace --all-targets

cd plugins/scema-web ; npm test        # 13 hermetic
# + 9 wire tests against a live daemon:
#   SCEMA_OMNID_URL=... SCEMA_OMNID_TOKEN=... npm test

cd ../../../web ; npm run check:omni   # 17 Rust↔TS parity checks
```

State lives in `.scema/` under the working directory: `decisions/<id>.json`,
`memory/*.jsonl`, and `omnid.token`. Gitignored — machine-local and full of absolute paths.

## What is not built

- **The action path.** Nothing here writes to an environment it observed. It needs the
  approval model `alchem-link` already worked out — risk declared per tool, no terminal
  means deny, secrets refused before the prompt — in front of it. Deliberately last: the
  loop is worth trusting with a keyboard only after it has been watched abstaining on real
  inputs for a while.
- **`delegate` / `discover` / `pay`.** Agent-to-agent hiring over the ScemaDEX relay needs a
  bonded result format; paying needs a spend policy first. A runtime that can spend without
  one is a runtime nobody should install.
- **A model-backed hypothesiser.** `HypothesisOrigin::Model` exists for it, and a model only
  *proposes* — the simulator still refuses to score an ungrounded branch. But its prompt,
  model id and raw output have to be committed into the record or determinism, and with it
  verifiability, is gone. A design question, not a wiring one.
- **Firefox.** The extension's perception module is portable; the MV3 background and
  `chrome.scripting` differences need their own pass.
