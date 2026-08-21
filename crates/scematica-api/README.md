# scematica-api

**The HTTP surface over a running Scematica bot.**

Serves what the sniper leaves on disk — metrics, filter statistics, trades, positions, the
DQ\* agent's state — to the Next.js dashboard in `web/` and the mobile companion. Read-only
for everything except the handful of control routes the dashboard uses to pause buys.

```console
$ cargo run --release --bin api
```

## Two endpoints worth knowing about

`GET /api/sentience` is the Ψ data-integrity gate, backed by `scematica-sentience`. It
answers the question every other endpoint quietly begs: **can anything reading this API
describe the bot right now?** Every read route serves a state file identically whether it
was written four seconds or four hours ago, and `/api/health` only reports that a process
*was* here. A `HOLD` verdict returns 409 rather than a stale-but-confident answer.

`GET /api/mesh` is the topology, collected by `scematica-mesh`: the decision-making units of
the running system, the edges between them, and — before any of that — whether each one can
be seen at all. It 503s when no bot is paired, which is **not** the same as an empty mesh: a
collector run against a directory with no state files returns a complete topology with every
node dark, and that is a true statement.

## The rule the whole crate follows

A missing file produces an absent reading, never a zero. `0 trades` and `cannot see the
executor` are different claims and only one of them accuses the system of idleness. Nothing
here fabricates a value to fill a field, and nothing simulates — the simulation branch lives
in `web/lib/sim/` where it is labelled, not in the process that is supposed to be reporting
the truth.

## Windows

Rebuilding `api.exe` while the API is running fails with `Access is denied (os error 5)`.
Cargo reports it as a build error rather than a lock error. Stop it first.

---

Part of [Scematica](https://github.com/Meta-Oracle/Scematica). Licensed MIT.
