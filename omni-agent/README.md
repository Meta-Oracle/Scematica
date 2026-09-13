# Scematica Omni-Agent

Scematica's field agent. It **reads live X discourse through Grok**, **judges it
with a neural network that learned your taste**, and acts across **X, Telegram
and the terminal from a single memory**.

Most posting agents are broadcasters: they generate on a timer from a persona
prompt, with no idea what the room is currently saying. This one runs a
perception loop instead, and the thing doing the judging is trained on your own
approve/reject decisions rather than on a prompt describing them.

```
  ┌─ PERCEIVE ──────────────────────────────────────────────┐
  │  Grok server-side x_search reads live X discourse        │
  └────────────────────────┬─────────────────────────────────┘
                           ▼
  ┌─ JUDGE ─ SCEMA CORTEX (PyTorch, CUDA) ───────────────────┐
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

## This is not Scematica Omni, and the distinction is load-bearing

Two things in this repository carry the word *omni* and they do different jobs.
Confusing them in public would lend an unsealed opinion the authority of a
sealed record, which is the one thing the other one exists to prevent.

| | `scematica-omni/` — **Omni**, the runtime | `omni-agent/` — **Omni-Agent**, this |
|---|---|---|
| What it does | observes a world, projects branches, **seals a verifiable decision record** | perceives live discourse, drafts prose, asks you |
| What it proves | a record was not edited after sealing | nothing — it is not a verifier |
| Its output | `.scema/decisions/<id>.json`, checkable offline at `/omni` | a draft in a queue, and a post if you approve one |
| Its judgement | an additive utility over measured terms, with coverage | a trained net, over your real decisions |
| Where it acts | `scema execute`, gated twice, dry-run by default | X, Telegram, the terminal |

The agent's character file says so in its own system prompt, because the place
that misstatement would actually be made is in a sentence it writes.

It is also **not** `scema-tgbot`, the Rust Telegram bot at
`crates/scematica-tgbot`. That one holds the authority to pause, dump and re-arm
the live sniper. This one holds none of it, and can only *read* the bot.

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
npx tsx src/cli.ts bot   # exactly what it can see of the live sniper
npm start         # full runtime: Telegram cockpit + scheduled sense loop
```

**Nothing reaches X until you explicitly enable it.** `SCEMA_DRY_RUN` defaults
to true and stays forced true while X credentials are missing; drafts land in
`data/queue/dry-run-posts.md` for review. The dry-run path runs the same queue
transitions and the same training, so the whole loop is exercisable before you
have a single credential.

## One bot, one poller

Telegram's `getUpdates` hands each update to **exactly one** caller. Two
processes polling one token do not both receive your commands — they split them
between them, at random, with no error anywhere. One of the two processes here
is `scema-tgbot`, which can sell positions, so this is not a cosmetic conflict.

Three variables, three bots, and the guard is structural rather than a warning:

| Variable | Whose bot | What happens |
|---|---|---|
| `SCEMA_AGENT_TG_TOKEN` | this cockpit's own | polls; approve / reject / edit / `/bot` |
| `SCEMA_TG_TOKEN` | `scema-tgbot`'s | read but **not polled**, unless `SCEMA_AGENT_TG_POLL=1` |
| `TELEGRAM_BOT_TOKEN` | a third, conversational | `@elizaos/plugin-telegram`; refused if it equals the cockpit's |

If two pollers do overlap anyway, Telegram answers HTTP 409 and the cockpit
raises `TelegramConflictError`, stops rather than retrying, and prints the fix.
Retrying would mean the two processes take turns stealing each other's commands,
which is worse than the cockpit being off.

## Reading the live bot

Scematica's processes talk to each other through JSON files in the bot's working
directory — no socket, no IPC channel. Point `SCEMA_BOT_DIR` at it and the agent
becomes a fifth reader of that surface, beside the ratatui dashboard, the HTTP
API, the web dashboard and `scema-tgbot`. Strictly read-only: no lock, no write.

`src/lib/bot-state.ts` exists because an agent that speaks in public about a
trading system has two bad options when asked how the bot is doing — decline
every such question, or produce a plausible number. So it reads, and it keeps
**three** states apart, never two:

- **absent** — no file. Not a bot that broke even. `0.00 SOL` would be a claim.
- **stale** — a file with an age on it. `scematica-metrics.json` is rewritten
  every 5 seconds, so an hour old means the sniper is stopped.
- **fresh** — a measurement, and the only case where a bare number may be spoken.

Nine tests in `bot-state.test.ts` assert what does *not* come out: no zero for an
unread PnL, no `0.0%` win rate over zero trades, no `0.000` for a DQ* field the
writer omitted. It is the em-dash rule the rest of this repository is built on,
restated here because nothing in this workspace can link to the Rust that owns it.

## Layout

```
src/
  character.ts              who the agent is, and what it may never claim
  cli.ts                    chat / sense / queue / bot / doctor
  index.ts                  ElizaOS runtime boot
  lib/queue.ts              event-sourced proposal queue (the audit trail)
  lib/bot-state.ts          the live sniper, read-only, absent ≠ stale ≠ fresh
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
npm test              # 61 TypeScript checks
npm run cortex:test   # 24 cortex checks (GPU)
npm run typecheck
```

The suites assert behaviour, not shape. `test_operator_taste_is_actually_learned`
fails if approve/reject stops changing predictions;
`test_context_features_do_not_become_a_shortcut` fails if the net starts
scoring by post length instead of meaning (it did once — see
`docs/DECISIONS.md`).

## Status

Working and verified end to end on this machine:

- Cortex: training, memory, persistence, HTTP surface — 24 checks on CUDA
- Decision → training loop: approve/reject through the real code path moves
  held-out taste separation to **0.999**
- Queue, dry-run posting, audit log, CLI, diagnostics, bot-state reader — 61 checks

Blocked on credentials, not code. Run `npx tsx src/cli.ts doctor` for the
live picture; as of the last check before the port into Scematica:

| Credential | Verdict | What it needs |
|---|---|---|
| xAI API key | **valid**, no credits | Add credits / raise the spending limit at console.x.ai. The API reports this as `permission-denied`, which reads like a bad key but is not. |
| X API key + secret | **valid** | Nothing. Verified by obtaining an app-only bearer. |
| X bearer token | **valid**, working | Verified live against `GET /2/tweets/20`: app-only reads succeed. It cannot post — X refuses user-context endpoints to application-only auth, which is what the OAuth 2.0 row below is for. |
| X access token pair (OAuth 1.0a) | **not set** | Optional. Generate it *after* setting the app to *Read and write* -- changing permissions afterwards silently invalidates existing tokens. The OAuth 2.0 row is the path that does not depend on it. |
| X OAuth 2.0 client id + secret | **set**, not yet authorised | Run `npm run x-auth`. This is the write path that does not depend on the broken access token pair, and its tokens refresh automatically. Register `http://localhost:3000/callback` as a Callback URI on the app first. |
| Telegram bot token | **not set for the cockpit** | The sniper's bot lives in the repository root's `.env`, and the cockpit will not poll it. Create a second bot with @BotFather and set `SCEMA_AGENT_TG_TOKEN`; then message it and run `doctor` for your chat id. |

Until xAI has credits, live search and generation are untested against the
real endpoint. Everything downstream of them is tested: the cortex, the
decision-to-training loop, the queue, dry-run posting, the bot-state reader and
the CLI.

See `docs/DECISIONS.md` for the choices that were made against measurement
rather than intuition, including the two bugs the tests caught.
