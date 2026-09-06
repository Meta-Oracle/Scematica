/-
  # Scematica, audited

  The machine-checked companion to `SECURITY-AUDIT.md`. Each module models one
  security-critical component of this repository and states, as theorems, both what the
  component guarantees and — where the audit found a defect — what it actually does.

  | Module | Component | Audit items |
  |---|---|---|
  | `Audit.Escrow` | `programs/scematica-escrow` | E-01, E-02, E-03, S-04 |
  | `Audit.Vault`  | `programs/scematica-vault`  | (all positive) |
  | `Audit.Swap`   | `programs/scematica-swap`   | W-01, W-02 |
  | `Audit.X402`   | `crates/scematica-protocol` | X-01, X-02, X-03 |
  | `Audit.Access` | `scema-daemon`, `scema-entitlement`, `crates/scematica-api` | A-01, A-02 |
  | `Audit.Effect` | `scema-effect`, `scema-cli execute` | EF-01 |

  A theorem whose name begins with `finding_` exhibits behaviour of the code as written; it
  is a defect witness, not a guarantee. Everything else is a property the implementation has
  and should keep.

  No dependencies, deliberately — see `scema-lean/README.md`, whose reasoning this library
  adopts. Build with `lake build Audit`.
-/
import Audit.Access
import Audit.Effect
import Audit.Escrow
import Audit.Swap
import Audit.Vault
import Audit.X402
