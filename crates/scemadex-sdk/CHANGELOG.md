# Changelog

All notable changes to `scemadex-sdk` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this crate adheres
to [Semantic Versioning](https://semver.org/).

## [0.2.0] - 2026-06-09

### Added — the adversarial layer (Primitives E–H)

Four new composable primitives, each offline-runnable with inline tests and an
example, extending the bond/mesh economy into a fully adversarial, self-policing
market for machine intelligence:

- **E · `counter` — the Counter-Market** (`CounterMarket`): adversarial
  conviction staking. Any agent can stake against an open bond; honored bonds
  pay forfeited stakes to the agent as a premium, slashed bonds pay challengers
  their stake plus a pro-rata share of the collateral. Exposes
  `market_conviction` (implied by stake) and `doubt_spread` (self-conviction
  minus market conviction) — a new policy feature. Example: `counter_market`.
- **F · `scar` — the Scar Market** (`certify_scar`, `ScarMarket`,
  `LocalScarMarket`): slash-certified failure trajectories as verified negative
  training data. Scars are only mintable from a `Slashed` settlement with
  non-zero collateral; buyers select maximum certified collateral per
  micro-USDC. Example: `scar_market`.
- **G · `lineage` — experience royalties** (`LineageLedger`): records which
  purchased `ExperienceBatch` trained which policy (`ExperienceBatch::digest()`
  is new) and streams a royalty slice of downstream fees pro-rata back to the
  sellers — training data as a yield-bearing asset. Self-sold batches earn
  nothing; value is conserved per split. Example: `experience_royalties`.
- **H · `teach` — bonded machine teaching** (`TeachingEngine`, `Teacher`,
  `ReferenceTeacher`): per-query metered distillation where the teacher bonds
  its tuition against the student's measured eval improvement; missed promises
  slash the bond as a tuition refund. Example: `bonded_teaching`.

## [0.1.4] - 2026-06-08

### Changed
- The optional `scematica` feature now depends on `scematica-nn` with
  `default-features = false`, so enabling it no longer pulls `scematica-nn`'s
  `cli` feature (and its `ratatui`/`crossterm` TUI stack) into your build. Only
  the Deep Q\* agent library is compiled.

## [0.1.3] - 2026-06-08

### Added
- **`scemadex` CLI** — `cargo install scemadex-sdk` now installs a `scemadex`
  binary: a ratatui live viewer that drives the lean core's bond engine + peer
  market through the full `intent → solve → conviction bond → settle` pipeline,
  fully offline (no RPC, keypair, or `solana-sdk`). Shipped behind a default
  `cli` feature; library-only consumers opt out with `default-features = false`
  to keep the lean trait surface with no TUI dependencies.

## [0.1.2] - 2026-06-08

### Added
- `conviction_client()` — a reference wiring backed by `EscrowBondEngine`, so the
  defining **Conviction Routing** primitive (D) is exercised end-to-end out of
  the box. The previous `reference_client()` uses `NoBondEngine` (a zero bond)
  and only demonstrates the intent/route surface.
- `examples/` for the four composing primitives, all runnable offline with no
  keypair, RPC, or `solana-sdk`:
  - `quote` — solve an intent into a bonded solution, then execute (A + B).
  - `conviction_bond` — conviction-weighted bonds, honored vs. slashed
    settlement, and the resulting honor-rate ledger (D + C).
  - `peer_market` — buy/sell bonded inferences and experience batches on the
    in-process mesh (the headline economy-of-intelligence loop).
  - `intent_solving` — the same trade under `Price` / `Speed` / `Stealth`
    objectives, showing the caller never specifies a path (B).

### Changed
- `docs.rs` now builds with `all-features` and `--cfg docsrs`, so the
  feature-gated `scematica`, `ai`, and `net` modules render with feature badges
  via `#[doc(cfg(...))]`.

## [0.1.0] - 2026-06-04

### Added
- Initial publish: the lean trait surface (`RoutePolicy`, `BondEngine`,
  `VenueExecutor`, `SignalSource`, `PeerMarket`) plus reference implementations
  (`ReferenceRoutePolicy`, `EscrowBondEngine`, `LocalPeerMarket`,
  `SimVenueExecutor`) and the `ScemaDex` facade. Optional `scematica`, `ai`, and
  `net` features.
