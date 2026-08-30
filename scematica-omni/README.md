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

**New here?** [GETTING-STARTED.md](GETTING-STARTED.md), or:

```console
$ cargo install scema-cli
$ cd ~/some-project && scema quickstart      # the loop, narrated. writes nothing
```

Five surfaces over one loop, and one entry point to all of them:

```console
$ cargo install scema-cli scema-tui scema-daemon scema-mcp

$ scema simulate "clear the marker backlog" --ground markers:scema-tools   # CLI
$ scema tui                                                                # console
$ scema daemon --allow .                                                   # loopback HTTP
$ scema mcp    --allow .                                                   # MCP, for models
$ scema connect claude-code --write                                        # wire it into an assistant
#   plugins/scema-web      → browser extension, the page as a world
#   plugins/claude-code    → a Claude Code plugin over the MCP server
#   /omni in web/          → verify a sealed record in a browser, offline
```

`scema` finds its siblings next to itself and hands over, so there is one command to
remember. They stay separate binaries so that installing the CLI on a CI machine that will
only ever run `scema verify` does not drag in a terminal stack.

Embedding the loop rather than running it:

```toml
[dependencies]
scema-agent  = "0.6"   # the whole loop
scema-world  = "0.6"   # just the types, if you only need the wire format
```

Beta. What is stable and what is not is spelled out in
[CHANGELOG.md](CHANGELOG.md#what-is-stable-in-beta-and-what-is-not) — the JSON contract, the
canonical encoding and `verify` are; the Rust trait shapes below the CLI are not yet.

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

## Four kinds of world, one loop

The claim "domain-agnostic" is cheap. What makes it true here is that a repository, a
running Solana bot, a set of Chainlink oracle feeds and a web page are all `WorldState`, and
nothing above perception can tell which it was looking at.

```console
$ scema observe .                                          # a source tree, in-process
$ mesh-dashboard --world | scema simulate "…" --path -      # a running bot
$ alchem-link omni -n base | scema simulate "…" --path -    # 66 Chainlink feeds
#   the browser extension posts a perceived page to the daemon
```

Only the first of those is a filesystem walk in Rust. The other three live behind a lockfile
pinned around `solana-sdk 1.18`, a stdlib-only Python package, and a browser — and linking
any of them would make `scema-tools` a hub of domain dependencies, which is the exact thing
the workspace note forbids.

So the arrangement is inverted: **the thing being observed describes itself in
`scema-world`'s vocabulary**, and `ImportObserver` reads that. There are four producers on
that contract and only one is written in a language omni's crates can link, which is why the
contract is a JSON shape rather than a trait.

| Producer | Language | Emits |
|---|---|---|
| `scema_tools::RepoObserver` | Rust, in-process | a source tree |
| `plugins/scema-web/src/perceive.js` | JavaScript, no build step | a DOM |
| `scematica_mesh::omni` | Rust, the bot workspace | a running Scematica system |
| `alchem_link.omni` | Python, stdlib only | one network's oracle feeds |

Two mechanisms keep three hand-written producers from drifting. Each **restates the
importer's validation on its own side** and fails its own tests — a magnitude outside
`[0,1]`, a duplicated signal id, and above all a signal claiming `measured: true` while
citing no evidence, which is a guess wearing a measurement's clothes. And
`crates/scema-tools/fixtures/` holds **real captured output** from all three, asserted
against the importer. A self-check catches a bug in one producer; a fixture catches the case
where both sides changed and only one of them was right.

An imported world's `observer` field is rewritten to `imported:<name>`, exactly as the
daemon rewrites a wire-supplied world to `client:<name>`. A record can never claim that a
world which arrived as a file was observed locally.

### Writing the fifth producer

Nothing about that list is privileged, and as of 0.5.0 nothing about it is hard-coded
either. The contract is versioned (`scema.world/1`) and both vocabularies are **open**:
`Domain` and `EntityKind` carry known names plus anything else, verbatim.

That change is what turns the claim into a mechanism. Before it, a perceived web page and a
set of Chainlink feeds both had to report `unknown` — two entirely different worlds,
indistinguishable to every specialist downstream — and naming a fifth kind of world meant
releasing this crate. Now a cluster, a dataset, a document corpus or a CI pipeline can say
what it is, and the runtime tells you honestly that its specialists will decline on it.

```console
$ my-tool --world | scema check -        # the importer's own rules, every failure at once
$ scema check --vocabulary               # the names this build knows
```

`scema check` runs `scema_tools::conform` — the same code the importer runs, not a
friendlier restatement. A checker that disagreed with the importer in either direction
teaches an author that the tooling is unreliable and to route around it. See
**[docs/PRODUCERS.md](docs/PRODUCERS.md)**.

### The mesh sees omni back

`scematica-mesh` now carries an `agent.omni` node, read from `.scema/decisions/`. It is the
only node in that topology with **no edges at all** — nothing in this workspace writes to an
environment it observed, so there is no wire into the buy path, and drawing one would assert
coordination that is not happening. It also counts records **without verifying them**, and
says so on the node: a second implementation of the canonical encoding is one that will
drift, and a verifier reporting an untampered record as INVALID teaches its reader to stop
believing it.

## Crates

| Crate | Role |
|---|---|
| `scema-world` | the universal state representation — `WorldState`, `Goal`, `Hypothesis`, `Term`, `Provenance`. Pure types, no I/O, `serde` only. Also the wire format for the browser extension. |
| `scema-tools` | perception. `Observer`, `RepoObserver`, and `ImportObserver` — a world perceived somewhere else. The only crate in the read path allowed to touch the outside world. |
| `scema-memory` | four memories — episodic, semantic, procedural, **counterfactual** — over append-only JSONL. |
| `scema-sim` | counterfactual projection. Refuses to invent a number. |
| `scema-policy` | `U = R − λ₁K − λ₂C − λ₃U + λ₄V`, plus pluggable evaluators that may decline. |
| `scema-verify` | canonical hashing and proof-carrying decision records. |
| `scema-trust` | whether an action may happen — risk per tool, a fixed preflight order, session-scoped grants. Checked against alchem-link's vectors. |
| `scema-anchor` | batch sealed records under one Merkle root with per-record inclusion proofs, so a commitment can be pinned somewhere its author does not control. |
| `scema-effect` | what the agent actually did — a sealed record of an attempted effect, with an explicit arm for a result nobody could observe. |
| `scema-nft` | a world drawn — a deterministic, self-contained SVG plate plus token metadata. Byte-identical to `web/lib/omni/nft.ts`. |
| `scema-agent` | the orchestration loop. |
| `scema-cli` | the `scema` binary — the loop, plus the launcher, `doctor` and `connect`. |
| `scema-tui` | `scema-tui` — the console. Black and violet, soft blue for a claim. |
| `scema-daemon` | `scema-omnid` — a loopback-only, token-authenticated HTTP surface, on `std`. |
| `scema-mcp` | the loop as MCP tools, over stdio, with workspace-confined paths. |

| Elsewhere | Role |
|---|---|
| `plugins/scema-web` | MV3 browser extension. Perceives a page as a `WorldState`. No build step. |
| `plugins/claude-code` | a Claude Code plugin: the MCP server, three commands, and a skill about reading the output. |
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

## The surfaces

All of them drive the same `scema-agent`. None re-implements perception, simulation or
verification, and none can reach a capability the others cannot — which is why the safety
argument only has to be made once.

### `scema` — the CLI

```console
$ scema quickstart                          # the loop, narrated. writes nothing
$ scema init                                # create .scema/, self-ignoring
$ scema observe .
$ scema simulate "clear the marker backlog" --ground markers:scema-tools
$ scema decide   "clear the marker backlog" --ground markers:scema-tools
$ scema explain 58898030 ; scema verify --all
$ scema policy
$ scema check world.json                    # does a producer's output conform, and why not
$ scema doctor                              # what is installed, wired, or quietly broken
$ scema connect --list                      # assistants this can wire the MCP server into
$ scema completions powershell
```

`simulate` never persists. `decide` seals a record and appends memory. `execute`,
`delegate`, `discover` and `pay` are registered and exit non-zero saying what is missing.

`quickstart` walks the whole loop over a directory you already have, explaining each stage
as it prints, and **stops before sealing** — a tutorial that writes a decision record on
your behalf has taught you the wrong thing about the one command here that leaves a trace.
It exists because two correct behaviours read as malfunctions on a first run: an ungrounded
goal abstaining, and a grounded signal branch outranking the goal you typed. Both now say so
and name the next command; neither ever fills in `--ground` for you.

`doctor` runs every check at once and **changes nothing** — each finding names the command
that would fix it and stops there. Its verdicts are four, not two: `ok`, `warn`, `FAIL`, and
`?` for a check that could not be run. "The record store does not verify" and "the record
store could not be read" are different claims, and only one of them is an accusation.

`connect` writes the MCP server into a project's `.mcp.json` / `.cursor/mcp.json` /
`.vscode/mcp.json`, merging rather than replacing — somebody routinely has three other
servers configured, and a tool that "adds" a fourth by rewriting the file deletes them.
User-level configs (Claude Desktop, Windsurf, Zed, Codex) are **printed with their path and
never written**: a user config is shared by every project you open, and editing it on your
behalf would mean a tool installed for one repository quietly gaining the ability to observe
all of them.

### `scema-tui` — the console

```console
$ scema tui                    # or: scema-tui /path/to/project
$ scema-tui --once             # one pass as plain text, pipeable
$ scema-tui --snapshot 120x40  # one frame as text, for a doc or a CI assertion
$ scema-tui --palette          # what colour this terminal can actually carry
```

Five tabs — WORLD, SIMULATE, RECORDS, MEMORY, POLICY — one per stage of the loop that
produces something a human needs to look at.

```
  1·WORLD │ 2·SIMULATE │ 3·RECORDS │ 4·MEMORY │ 5·POLICY │        SCEMA OMNI scema-omni/0.1.0
┌ SIMULATION MATRIX  ·  NOT WRITTEN (would seal as c5ac7c5e) ────────────────────────────┐
│ #   BRANCH                              GAIN   RISK   COST UNCERT REVERS  UTILITY  MEAS │
│▸  1 take: 11 marker(s) in `scema-tools` 0.22   0.40   0.10   0.00   0.70    0.125  ▰▰▰▰▰│
│   2 give scema-cli tests                   —   0.40   0.10   0.00   0.70    0.093  ▰▰▰▰▱│
└─────────────────────────────────────────────────────────────────────────────────────────┘
```

Four things about it are load-bearing rather than cosmetic:

* **Black and violet, with soft blue for exactly one thing.** Every other terminal surface
  in this repository has its own identity — the sniper dashboard is black and red,
  `mesh-dashboard` is indigo over slate. An operator with three open must be able to tell at
  a glance which is making a claim about their money and which about a decision record.
  Azure appears only on the branch that was *chosen* and on a commitment that *verifies*: an
  accent that appears on every third row is not an accent, it is a second body colour.
* **A renderer names a role, never a colour**, and **colour is never the message**.
  `--no-color`, a pipe and a 16-colour terminal all produce the same text — `—` for
  unmeasured, `▸` for chosen, `LIVE`/`STALE`/`ABSENT`, `EXCLUDED`. A test asserts that every
  role still carries a modifier when there is no colour at all, so nothing can be
  distinguishable by hue alone.
* **The coverage meter is a count, not a bar.** One cell per term, `▰▰▰▱▱`. A proportional
  bar would render `2/5` and `4/10` identically, and the denominator is the number that
  matters.
* **`enter` simulates; `D` decides.** Two keys, and a confirmation on the second, because
  the two paths compute *exactly* the same thing and differ only in whether they leave a
  trace. The only protection against a counterfactual later reading as a decision is that
  they are not the same keystroke.

`--snapshot` draws a frame into an off-screen buffer and prints it, which is how a TUI gets
tests at all: layout arithmetic underflows on small rectangles, and the production failure
mode is the console dying on somebody's 80-column terminal.

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

It reads nothing until you ask: no `content_scripts` block, no `<all_urls>`, injection via
`activeTab` from the popup's button or `Alt+Shift+O`. The token lives only in the service
worker, never in the content script, and the content script picks a message *type* rather
than a URL. The one route that needs a caller-supplied segment — `GET /decisions/{id}` —
validates the id against a pattern before the path is built, so a `../` never reaches the
daemon's router.

Its palette is a port of the console's, pinned by a test, so the two surfaces cannot drift
into looking like different products. And it deliberately **verifies nothing itself**: a
fourth implementation of the canonical encoding is one that will drift, and an overlay
reporting an untampered record as INVALID is the most damaging failure available. There is
an export button instead, and the exported bytes are identical to what `RecordStore::save`
wrote.

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

### `plugins/claude-code` — the assistant plugin

```console
> /plugin marketplace add Meta-Oracle/Scematica
> /plugin install scema-omni@scematica
```

The MCP server can be added with one line of JSON. What it cannot do by itself is stop a
model from writing *"expected gain: 0.00"* when the tool said `—`.

That is the whole reason the plugin carries a skill rather than only a config file. Omni's
design is that every layer can say "I don't know" — `Provenance` before value, `Term` before
score, `Applicability` before opinion — and the last layer is prose written by a model. A
summary that reports an unmeasured term as a zero has undone the type system underneath it
in one sentence, and nothing downstream can tell.

The skill is five things not to do, each a failure this repository has paid for at least
once: an em dash is not a zero; coverage never leaves the score it qualifies; abstention is
an answer and *which* one is the actionable part; grounding is asserted, never inferred from
wording; a verified commitment proves one thing and not two others.

There is deliberately no `--allow-decide` in the plugin's `.mcp.json`, so `omni_decide` is
not advertised at all — absent rather than listed-and-failing, which teaches a model to
retry.

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

It also **draws** the record's world, with the same code the CLI uses. See below.

### `scema nft` — a world, drawn

```bash
scema observe . --json | scema nft - --out plate.svg --metadata plate.json
scema nft .scema/decisions/<id>.json --out plate.svg
```

A `WorldState` rendered as a self-contained SVG — no fonts to fetch, no image host, no
script — with ERC-721-shaped token metadata alongside it. `/omni` produces the identical
file in the browser from a dropped record, and offers both as downloads.

**Identical is meant literally.** `web/lib/omni/nft.ts` is a port of `scema-nft`, and unlike
the render rule in `view.ts` — where three implementations share a *rule* and each is tested
separately — these two must emit the **same bytes**. An image that depends on which runtime
drew it is not a derivative of anything: the CLI and the browser would produce two different
artefacts for one world. `check:omni` compares against a fixture carrying Rust's output and
fails on one differing character.

That is not achievable by care, which this repository learned in `canonical.rs`. There is no
trigonometry (`sin`/`cos` are not correctly rounded by IEEE-754 and may differ in the last
place — both sides index the same integer sine table at whole degrees), no decimal
formatting of floats (`{:.3}` and `toFixed(3)` break ties differently — coordinates are
integers in thousandths of a unit), rounding is half away from zero spelled out on both
sides, text is measured in code points, and base64 encodes UTF-8 bytes rather than going
through `btoa`. There is also **no clock**: a "minted at" field would make every
regeneration a different token.

The plate is an instrument. The rule that shapes it is the em-dash rule in vector form —
**an unmeasured gauge must not look like a measured zero** — because both would otherwise be
a zero-length arc, which is the same picture. So a gauge nobody measured draws its full
sweep *dashed*, and a gauge measured at zero draws *nothing* and prints `0.00`. Blind spots
cut visible notches through the extent ring, because ignorance should be a hole rather than
blank space. Estimated magnitudes get hollow caps. Coverage is one cell per signal, never a
proportional bar.

The sharpest case is `legibility`, which returns `0.0` both for a world whose objects are
all unreadable and for a world with no objects at all — `world.rs` says so in its own doc
comment. The number cannot tell them apart; the picture must, so nothing-to-read draws a
dashed ghost outline and `∅` while a measured zero draws no disc and prints `0.00`.

Two things it does not do. It **does not score the world** — no rarity, tier or rank, and
both test suites assert the absence, because a ranking invented here would be a number of
exactly the right shape with nothing behind it, laundered through a signed artefact. And it
**does not mint, sign or spend**: it writes files, and where they go next is your decision,
not this runtime's.

Handed a sealed record it uses the *stored* `commitment.world` rather than recomputing one,
so an edited record produces a plate whose digest does not match its own world. That
mismatch is the tamper signal; recomputing would quietly repair the evidence.

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
cargo test --workspace                 # 262 tests
cargo clippy --workspace --all-targets

cd plugins/scema-web ; npm test        # 45 hermetic
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
