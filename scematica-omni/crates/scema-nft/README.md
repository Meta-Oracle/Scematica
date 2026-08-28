# scema-nft

A Scematica Omni `WorldState`, drawn — as a self-contained SVG plate with ERC-721-shaped
token metadata.

```bash
scema observe . --json | scema nft - --out plate.svg --metadata plate.json
scema nft .scema/decisions/<id>.json --out plate.svg
```

The same world always produces the same bytes, in this crate and in `web/lib/omni/nft.ts`.
That is what makes the plate a *derivative of the record* rather than an illustration of
it: anybody holding the world file can regenerate the image and get the same artefact.

## What the plate says

It is an instrument, not decoration. Four rules and no legend needed:

| Mark | Means |
|---|---|
| Outer ring, solid arc | extent — how much of the entity the observer reached |
| Outer ring, **dashed all the way round** | the observer does not know the denominator |
| A **notch** through the outer ring | a blind spot: something it tried to read and could not |
| Spoke length | signal magnitude · **triangle** = risk, **disc** = opportunity |
| **Hollow** cap, dashed spoke | the magnitude was estimated, not counted |
| Inner ring | provenance mix; `absent` and `simulated` are dashed |
| Core disc | legibility — the share of objects that may be acted on |
| Core `∅` | there was nothing to read, which is not the same as nothing being readable |
| Footer cells | coverage, one cell per signal — never a proportional bar |

The rule underneath all of them is the one this workspace is built on:

> **An unmeasured gauge must not look like a measured zero.**

A gauge nobody measured draws its full sweep dashed and labels itself `—`. A gauge measured
at zero draws *nothing* and labels itself `0.00`. Both would otherwise be a zero-length arc,
which is to say the same picture — the em-dash rule of `scema_policy::render` in vector form.

`WorldState::legibility` returns `0.0` for two different worlds: one where objects were
observed and none are actionable, and one where there were no objects at all. The number
cannot tell them apart. The picture does.

## Determinism, and what it forbids

Byte-identical output across Rust and a browser is a hard requirement. It is also not
achievable by care — this repository already learned that in `scema_verify::canonical`. So:

- **No trigonometry.** `sin`/`cos` are not correctly rounded by IEEE-754 and may differ in
  the last place between runtimes; a one-ULP difference survives rounding at every tie.
  Both sides index the same integer sine table at whole degrees.
- **No decimal formatting of floats.** `{:.3}` and `toFixed(3)` break ties differently.
  Coordinates are integers in thousandths of a unit, formatted by integer arithmetic.
- **Rounding is half away from zero**, spelled out on both sides, because `Math.round`
  rounds half toward positive infinity.
- **Text is measured in code points**, and base64 encodes UTF-8 bytes rather than going
  through `btoa`.
- **No clock and no randomness.** There is deliberately no "minted at" field: a timestamp
  taken at render time would make every regeneration a different token.

`cargo test -p scema-nft` writes `fixtures/` and fails if they drifted;
`npm run check:omni` compares the TypeScript port against those files.

## What it does not do

**It does not score the world.** No rarity, no tier, no rank, no quality out of ten. Every
quantity drawn is one an observer counted; a ranking invented here would be a number of
exactly the right shape with nothing behind it, laundered through a signed artefact into
somebody's wallet. Both test suites assert the absence.

**It does not mint, sign or spend.** It writes files. Where they go next is your decision.

**It does not re-verify a record.** Handed one, it uses the *stored* `commitment.world`, so
an edited record produces a plate whose digest does not match its own world — which is the
tamper signal. `scema verify` is what checks it.

## What the commitment proves

The digest on the plate is the world's canonical commitment, computed by the same code that
seals a decision record.

- It **does** bind this picture to that exact world file.
- It does **not** prove the world was as described — provenance carries that, which is why
  the plate draws provenance rather than hiding it.
- It does **not** prove this is the only plate for that world. Tamper-evident, not
  tamper-proof, until the root is anchored somewhere the author does not control.

All three travel in the token description, not only in this README.
