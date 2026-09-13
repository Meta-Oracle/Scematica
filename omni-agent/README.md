# Scematica Omni-Agent

An agent that **reads live X discourse through Grok**, **judges it with a neural
network that learned your taste**, and acts across **X, Telegram and the
terminal from a single memory**.

Most posting agents are broadcasters: they generate on a timer from a persona
prompt, with no idea what the room is currently saying. Omni runs a perception
loop instead, and the thing doing the judging is trained on your own
approve/reject decisions rather than on a prompt describing them.

```
  ┌─ PERCEIVE ──────────────────────────────────────────────┐
  │  Grok server-side x_search reads live X discourse        │
  └────────────────────────┬─────────────────────────────────┘
                           ▼
  ┌─ JUDGE ─ OMNI CORTEX (PyTorch, CUDA) ────────────────────┐
  │  TasteNet, one trunk, three heads:                       │
  │    salience  → does this matter at all                   │
  │    taste     → would the operator approve this           │
  │    resonance → how much engagement will it earn          │
  └────────────────────────┬─────────────────────────────────┘
                           ▼
  ┌─ COMPOSE ── draft, with cross-surface memory in context ─┐
  └────────────────────────┬─────────────────────────────────┘
                           ▼
  ┌─ DISPATCH ── Telegram: ✅ post  ❌ reject  ✏️ edit ───────┐
  └────────────────────────┬─────────────────────────────────┘
                           ▼
  ┌─ REFLECT ── engagement measured, fed back as labels ─────┐
  └────────────────────────┴──── trains the cortex ──────────┘
```

The loop closes. Your Telegram decisions **are** the training set; measured
engagement **is** the resonance label. The network is not decoration — the
sense loop cannot rank without it.

## Why this is not just another ElizaOS character

| | Typical posting agent | Omni |
|---|---|---|
| Knows what's being said now | No — training data | Yes — Grok `x_search` at inference |
| Decides what's worth saying | Prompt says "be interesting" | Trained salience head |
| Learns your preferences | You rewrite the prompt | Trained taste head, from real decisions |
| Avoids repeating itself | No | Novelty scored against vector memory |
| Memory across platforms | Per-room scrollback | One store, every surface |
| Autonomy | On or off | Earned — gated on measured agreement |

## Quick start

```bash
npm install
cp .env.example .env          # add XAI_API_KEY at minimum

npm run cortex                # terminal 1: the neural sidecar
npx tsx src/cli.ts doctor     # terminal 2: what works, what doesn't
```

`doctor` tells you exactly what is missing and how to fix it. Nothing is
required to start — the agent runs degraded and says so.

```bash
npm run x-auth    # authorise X posting (OAuth 2.0 browser flow)
npm run chat      # talk to it (live search + memory)
npm run sense     # run one perception cycle now
npm run queue     # review drafts: approve / reject / edit
npm start         # full runtime: Telegram cockpit + scheduled sense loop
```

**Nothing reaches X until you explicitly enable it.** `SCEMA_DRY_RUN` defaults
to true and stays forced true while X credentials are missing; drafts land in
`data/queue/dry-run-posts.md` for review. The dry-run path runs the same queue
transitions and the same training, so the whole loop is exercisable before you
have a single credential.

## Layout

```
src/
  character.ts              who the agent is
  cli.ts                    chat / sense / queue / doctor
  index.ts                  ElizaOS runtime boot
  lib/queue.ts              event-sourced proposal queue (the audit trail)
  plugins/
    grok/                   xAI models + live x_search  (no official plugin exists)
    cortex/                 bridge to the neural core: memory provider + evaluator
    sense-loop/             the perception cycle
    control-plane/          Telegram approve / reject / edit cockpit
    twitter/                the one place a real post can be emitted
cortex/                     Python neural core
  scema_cortex/
    model.py                TasteNet: 960k params, 3 heads, FeatureNorm
    train.py                online training, replay buffer, class balancing
    store.py                vector memory with recency-weighted recall
    kernels/                top-k cosine: mojo | numpy | torch
    server.py               FastAPI sidecar
```

## The Mojo question

`cortex/scema_cortex/kernels/similarity.mojo` is a real SIMD kernel for the one
hot loop in the system (top-k cosine, used for novelty on every candidate).
**It is not the active path on this machine, and honesty requires saying why:
Modular ships no native Windows toolchain for Mojo.** Build it under WSL with
the steps in `kernels/README.md`; the loader picks it up automatically.

The active kernel is **numpy**, chosen by measurement, not preference:

```
        n         numpy         torch
   100,000      8.160ms      28.284ms      → torch is 0.24x (slower)
```

The GPU loses because each call copies the whole memory matrix host→device.
The GPU earns its place on batched TasteNet inference and training instead,
where data is already resident. Re-measure on your hardware with
`npm run cortex:bench`.

## Testing

```bash
npm test              # 28 TypeScript checks
npm run cortex:test   # 24 cortex checks (GPU)
npm run typecheck
```

The suites assert behaviour, not shape. `test_operator_taste_is_actually_learned`
fails if approve/reject stops changing predictions;
`test_context_features_do_not_become_a_shortcut` fails if the net starts
scoring by post length instead of meaning (it did once — see
`docs/DECISIONS.md`).

## Status

Working and verified end to end:

- Cortex: training, memory, persistence, HTTP surface — 24 checks on CUDA
- Decision → training loop: approve/reject through the real code path moves
  held-out taste separation to **0.999**
- Queue, dry-run posting, audit log, CLI, diagnostics — 28 checks
- Grok client: retry policy and tolerant response parsing — offline tests

Blocked on credentials, not code. Run `npx tsx src/cli.ts doctor` for the
live picture; as of the last check:

| Credential | Verdict | What it needs |
|---|---|---|
| xAI API key | **valid**, no credits | Add credits / raise the spending limit at console.x.ai. The API reports this as `permission-denied`, which reads like a bad key but is not. |
| X API key + secret | **valid** | Nothing. Verified by obtaining an app-only bearer. |
| X bearer token | **valid**, no credits | The X project reports HTTP 402 `credits depleted`. |
| X access token pair (OAuth 1.0a) | **rejected** (code 89) | Regenerate *after* setting the app to *Read and write* -- changing permissions afterwards silently invalidates existing tokens. |
| X OAuth 2.0 client id + secret | **set**, not yet authorised | Run `npm run x-auth`. This is the write path that does not depend on the broken access token pair, and its tokens refresh automatically. Register `http://localhost:3000/callback` as a Callback URI on the app first. |
| Telegram bot token | set | `SCEMA_TG_OPERATOR_CHAT_ID` is unset, so the cockpit is read-only. Message the bot, then run `doctor`. |

Until xAI has credits, live search and generation are untested against the
real endpoint. Everything downstream of them is tested: the cortex, the
decision-to-training loop, the queue, dry-run posting and the CLI.

See `docs/DECISIONS.md` for the choices that were made against measurement
rather than intuition, including the two bugs the tests caught.
