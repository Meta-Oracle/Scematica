# Changelog

All notable changes to `scemadex-sdk` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this crate adheres
to [Semantic Versioning](https://semver.org/).

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
