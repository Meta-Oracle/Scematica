# Decisions

Choices that were made against measurement rather than intuition, and the
bugs that changed the design. Recorded because the reasoning is the part that
does not survive in the code.

---

## 1. BatchNorm destroyed generalisation; FeatureNorm fixed it

**Observed.** Training the taste head on operator decisions drove training loss
to 0.0003 while held-out separation was **+0.019** — effectively nothing. The
offline unit test, meanwhile, showed perfect separation (+1.000). Same model,
same data, opposite results.

**Isolated.** The difference was the context feature vector. The offline test
passed all-zero features; the service passed real ones. Three runs:

```
features all zero                 train_loss=0.013  good=1.0000 bad=0.0000  sep=+1.0000
features as the service sends     train_loss=0.012  good=1.0000 bad=1.0000  sep=+0.0000
novelty only (constant 1.0)       train_loss=0.013  good=1.0000 bad=0.0000  sep=+1.0000
```

A *constant* feature was harmless. `text_len_norm` — which varies slightly and
correlated with the label by accident in that training set — collapsed
separation to zero.

**Cause.** `nn.BatchNorm1d` divides by the batch standard deviation. Across the
tiny, homogeneous batches online training produces, most context features
barely vary, so their noise was amplified into a dominant signal. The net
learned the shortcut "longer post = approved" and stopped reading the text.
Training loss looked excellent throughout, because the shortcut fits the
training set perfectly.

**Fix.** `FeatureNorm`: running statistics with a **variance floor** (a feature
with no real spread contributes ~0 rather than exploded noise), always applied
in inference mode so train and eval never diverge, plus **feature-block
dropout** (25% of rows train with features blanked, forcing the trunk to read
the embedding).

**Result.** Separation restored to +1.0000; the length-shortcut probe went to
0.0000 delta. Locked in by `test_context_features_do_not_become_a_shortcut`,
whose labels correlate with length *by construction* so the trap is always set.

**Generalisable lesson.** Falling training loss is not evidence of learning. The
only test that caught this scored held-out examples.

---

## 2. The GPU is not the fast path for retrieval

**Assumed.** CUDA is available, so run top-k cosine on the GPU.

**Measured** (`npm run cortex:bench`, RTX 2070 SUPER, d=384, k=8):

```
        n         numpy         torch   agree
    1,000      0.040ms      0.520ms   yes
   10,000      0.958ms      3.110ms   yes
  100,000      8.160ms     28.284ms   yes
```

torch is **0.24x** — four times slower. Every call copies the entire memory
matrix host→device, and that copy dwarfs the matmul it enables. Memory rows are
owned by numpy on the host, so the copy is per-call.

**Decision.** Default to numpy; keep torch behind `SCEMA_KERNEL=torch` for the
case where memory grows enough to amortise the transfer. The GPU is used for
batched TasteNet inference and training, where the data is already resident.

This is also the argument *for* the Mojo kernel: it works in-place on the host
buffer with no copy and no allocation, which is exactly the cost that beats the
GPU here.

**Generalisable lesson.** The benchmark was written to justify a kernel choice
and immediately falsified it. Write the benchmark before the defaults.

---

## 3. `np.savez_compressed` silently renamed the checkpoint

Memory save/load round-tripped to **zero records**. `np.savez_compressed`
appends `.npz` to any path that lacks it, so writing to `memory.npz.tmp`
produced `memory.npz.tmp.npz`; the atomic `tmp.replace(path)` then failed on a
file that did not exist, and the error was swallowed by the `OSError` handler.

Fixed by passing an open file handle, which suppresses the rename. Caught by
`test_save_and_load_round_trip`.

---

## 4. `.filter(Boolean)` ate the review log's formatting

The dry-run review log is the only output a dry-run operator reads. Building
each entry as an array and calling `.filter(Boolean)` to drop one optional
field also dropped every intentional blank-line separator, so entries ran
together as `---## 2026-09-13T...` — 14 of 15 collided.

Fixed by pushing the optional line conditionally instead of filtering the whole
array. Caught by the end-to-end integration run, not by a unit test, and now
locked in by `post.test.ts`.

**Generalisable lesson.** `filter(Boolean)` does not mean "drop the optional
one". It means "drop everything falsy", including the empty strings that are
load-bearing.

---

## 5. Embeddings go through the cortex, not xAI

ElizaOS memory and cortex memory must occupy the same vector space, or "what do
I remember about this" gives different answers depending on which subsystem
asks. `ModelType.TEXT_EMBEDDING` is therefore routed to the cortex's `/embed`.

On failure it returns **zero vectors, not hashed stand-ins**. A zero vector is
cosine-neutral to everything, so retrieval degrades to "no opinion". A
fabricated vector would be confidently wrong and would poison the stored index
permanently.

---

## 6. Operator decisions are buffered to disk, never dropped

The cortex is a separate process that can be down. Scoring degrades to neutral
and recall degrades to empty, but feedback is written to
`data/cortex-pending-feedback.jsonl` and replayed on reconnect.

Labels are the scarcest resource in the system — a human pressed a button to
produce each one. Losing them silently would hollow out the entire premise
while everything still appeared to work.

---

## 7. Autonomy is earned, not configured

Auto-posting requires three independent gates: the operator opted in
(`SCEMA_SENSE_AUTOPOST_TASTE` ≤ 1.0), the cortex has seen at least
`SCEMA_SENSE_AUTOPOST_MIN_EVENTS` real decisions, and the specific draft clears
the taste threshold. Default is never.

An untrained network has no opinion worth acting on, and a confident-looking
0.97 from a net with twelve labels behind it is noise. The gate is on
*evidence*, not just confidence.

---

## 8. The control plane bypasses the conversational pipeline

Approvals go through the Telegram Bot API directly rather than through
`@elizaos/plugin-telegram`. That plugin turns messages into agent turns, which
would put an LLM between the operator pressing "reject" and the rejection being
recorded.

A control surface needs determinism. Both run against the same token: the
plugin handles the message side, the cockpit handles callback queries.

---

## 9. There is no official Grok plugin, so this one is ours

`@elizaos/plugin-grok` does not exist on npm. `@elizaos/plugin-xai@2.0.0-alpha.1`
declares `"@elizaos/core": "workspace:*"`, which cannot resolve outside the
Modular monorepo. Version facts as of 2026-09-13:

| package | version |
|---|---|
| `@elizaos/core`, `cli`, `bootstrap`, `sql` | 1.7.2 |
| `@elizaos/plugin-telegram` | 1.6.4 |
| `@elizaos/plugin-twitter` | 1.2.22 (pins core `^1.6.3`, uses `twitter-api-v2`) |

`plugin-twitter` speaking `twitter-api-v2` is the consequential one: it uses the
**official X API with OAuth 1.0a**. Scraper-era `TWITTER_USERNAME` /
`TWITTER_PASSWORD` credentials cannot drive it, whatever older guides say.

---

## 10. Mojo cannot be built or verified on this machine

Modular ships Mojo for Linux and macOS only. The `mojo` on PATH here is Perl's
Mojolicious, from Strawberry Perl.

`similarity.mojo` is written and its ctypes ABI is fixed, but it has **not been
compiled**, so treat the build as unverified. Two surfaces are version-sensitive
and are the likely failure points: the `@export` decorator (v1.0.0 confirms
`mojo build --emit shared-lib` but does not document the decorator spelling) and
`Pointer[mut=False, Scalar[dtype]]` (v1.0.0 unified `Pointer`/`UnsafePointer`).
The arithmetic between them is plain arithmetic.

A failed build costs throughput, never correctness: the loader catches it and
falls through to numpy, and `bench.py` asserts every kernel returns identical
results before any of them is trusted.
