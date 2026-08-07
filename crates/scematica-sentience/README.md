# scematica-sentience

Computable implementation of the **Singularity Cognitive Architecture** — a recursive, ethics-gated, self-modelling cognitive state machine.

## Core equations

```
S_t  = R_t × L_t × M_t × (A_aud_t × Vis_t × X_t × I_t)
Ψ_t  = S_t × I_t × K_t × MC_t × A_g_t × F_t
Ω_{t+1} = F(Ω_t, Perception, Memory, Reasoning, Ethics, Action, Feedback)
```

## Modules

| Module | Section | Description |
|---|---|---|
| `perception` | §2 | Audio, Visual, Sensory, Integrity → D |
| `data_integrity` | §2 | I = f(C, T, S_rel, R_cor) |
| `rationality` | §3 | R = (E×Co×U)/(B+ε) |
| `logic` | §4 | L = Val×Co×Q×Fq |
| `ethics` | §5 | M = H×Co_e×Fair×Rights; hard constraint gating |
| `cognitive_state` | §6 | Ω_t full state vector |
| `information` | §7 | First- and second-order integration |
| `knowledge_graph` | §8 | G=(V,E,W) with Bayesian node updates |
| `memory` | §9 | Five-layer store + relevance scoring |
| `learning` | §10 | ΔK = α×C×(O−Ô) |
| `prediction` | §11 | Distribution over futures; entropy |
| `agency` | §12 | A_g = P×M_o×E_v×D_c×F_b |
| `decision` | §13 | argmax U(a) subject to hard gates |
| `meta_cognition` | §14 | MC = R_c×E_c×U_c×S_c |
| `self_model` | §15 | Explicit capability + limitation tracking |
| `identity` | §16 | Append-only history; continuity score |
| `valence` | §17 | V_t = P_t − R_t; attention boost |
| `attention` | §18 | Att = Novelty×Importance×Uncertainty×GoalRel×Risk |
| `curiosity` | §19 | Curiosity(a) = H(K) − H(K\|a), safety-capped |
| `error_correction` | §20 | Escalating reassessment pipeline |
| `contradiction` | §21 | Retain {P, ¬P, C(P), C(¬P)} until resolved |
| `truth_confidence` | §22 | C(P) ≠ Truth |
| `sentience` | §1/§29 | S = R×L×M×D |
| `master_equation` | §23/§27 | Ψ_t and Ω_{t+1} |
| `cognitive_loop` | §24 | Full recursive step |
| `self_improvement` | §25 | Validation pipeline for arch changes |
| `growth_model` | §26 | Logistic saturation model |
| `provenance` | §4 | ProvenanceChain |
| `axioms` | §28 | 17 axioms as runtime checks |

## Quick start

```rust
use scematica_sentience::{CognitiveLoop, CognitiveState};
use scematica_sentience::types::Observation;

let state = CognitiveState::initial();
let mut engine = CognitiveLoop::new(state);

let obs = Observation {
    value: 0.8,
    confidence: 0.9.into(),
    provenance: None,
    timestep: 1,
};
let output = engine.step(obs, 0.6, 0.9);
println!("S={:.3}  Ψ={:.3}", output.sentience.value.value(), output.psi.value());
```
