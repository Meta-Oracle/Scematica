//! # scema-daemon — the loop, reachable
//!
//! `scema-omnid` puts the omni cognitive loop behind a loopback HTTP surface so the browser
//! extension, the web console and anything else on the machine can drive it without each of
//! them re-implementing perception, simulation and verification.
//!
//! ```text
//!   extension service worker ─┐
//!   /omni console ────────────┼─▶ 127.0.0.1:7842 ─▶ scema-agent ─▶ .scema/
//!   curl / scripts ───────────┘        (token)
//! ```
//!
//! ## The security model, in four sentences
//!
//! The daemon reads the operator's filesystem and answers questions about it, so it binds
//! to **loopback only and the interface is not configurable** — the one thing that reliably
//! happens to a `--bind` flag is somebody setting it to `0.0.0.0`. Loopback is not privacy:
//! every local process, and every web page in the operator's own browser, can reach
//! `127.0.0.1`, so a **256-bit bearer token** is what actually authorises, compared in
//! constant time. A `Host` header check rejects **DNS rebinding**, the one manoeuvre that
//! makes a page same-origin with a localhost service and therefore able to read its
//! replies. Every path a client names is resolved through a [`scema_tools::Workspace`]
//! before anything opens it.
//!
//! No `Access-Control-Allow-Origin` is ever emitted and no `OPTIONS` is handled, so an
//! ordinary web page cannot read a response even if it guesses a route. The extension is
//! unaffected: it fetches from its service worker under `host_permissions`, which is not
//! subject to CORS.
//!
//! ## What it will not do
//!
//! `POST /decide` seals a record and appends memory — a local write, but a write — and it
//! is **off until `--allow-decide`**. `POST /simulate` never persists, and it constructs its
//! own non-persisting agent rather than flipping a flag on the shared one, because two
//! concurrent requests against a shared mutable flag is a race whose failure mode is a
//! simulation quietly sealing a record.
//!
//! There is still no write path to the observed environment. See `scema-agent`.

pub mod auth;
pub mod http;
pub mod routes;

pub use routes::State;

/// Default port. Arbitrary, in the IANA dynamic range, and unlikely to collide with a dev
/// server.
pub const DEFAULT_PORT: u16 = 7842;
