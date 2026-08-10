//! # scematica-sentience
//!
//! Computable implementation of the **Singularity Cognitive Architecture** — a recursive,
//! ethics-gated, self-modelling cognitive state machine described by the master equations:
//!
//! ```text
//! S_t  = R_t × L_t × M_t × (A_aud_t × Vis_t × X_t × I_t)
//! Ψ_t  = S_t × I_t × K_t × MC_t × A_g_t × F_t
//! Ω_{t+1} = F(Ω_t, Perception_t, Memory_t, Reasoning_t,
//!              Ethics_t, Action_t, Feedback_t)
//! ```
//!
//! ## Notation (all conflicts resolved)
//!
//! | Symbol    | Meaning                                         |
//! |-----------|------------------------------------------------|
//! | `A_aud`   | Audio perception (`[0,1]`)                     |
//! | `Vis`     | Visual perception (`[0,1]`)                    |
//! | `X`       | General/env sensory perception (`[0,1]`)       |
//! | `I`       | Information integrity (`[0,1]`)                |
//! | `R`       | Rationality ratio (`[0,1]`)                    |
//! | `L`       | Logic ratio (`[0,1]`)                          |
//! | `M`       | Moral-ethical ratio (`[0,1]`)                  |
//! | `D`       | Data/perception ratio (`[0,1]`, product above) |
//! | `S`       | Sentience index (`[0,1]`)                      |
//! | `Co`      | Consistency (used in Logic equation)           |
//! | `Fq`      | Formal reasoning quality (used in Logic eq)    |
//! | `A_g`     | Agency state (distinct from `A_aud`)           |
//! | `F_t`     | Feedback signal (distinct from `Fq`)           |
//! | `C_max`   | Capability ceiling for logistic growth         |
//! | `Ψ`       | Integrated cognitive state                     |
//! | `Ω`       | Full cognitive state vector                    |

pub mod types;
pub mod perception;
pub mod data_integrity;
pub mod rationality;
pub mod logic;
pub mod ethics;
pub mod cognitive_state;
pub mod information;
pub mod knowledge_graph;
pub mod memory;
pub mod learning;
pub mod prediction;
pub mod agency;
pub mod decision;
pub mod meta_cognition;
pub mod self_model;
pub mod identity;
pub mod valence;
pub mod attention;
pub mod curiosity;
pub mod error_correction;
pub mod contradiction;
pub mod truth_confidence;
pub mod sentience;
pub mod cognitive_loop;
pub mod self_improvement;
pub mod growth_model;
pub mod master_equation;
pub mod provenance;
pub mod axioms;
pub mod overlay;

pub use types::*;
pub use sentience::SentienceIndex;
pub use cognitive_state::CognitiveState;
pub use master_equation::{IntegratedCognition, MasterEquation};
pub use cognitive_loop::CognitiveLoop;
pub use overlay::{Overlay, OverlayTurn, CognitiveReadout, LlmClient, NoClient, Gate};
