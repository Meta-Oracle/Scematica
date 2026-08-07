//! §9  Memory Equation
//!
//! ```text
//! M_total = M_sensory + M_working + M_episodic + M_semantic + M_procedural
//!
//! R_m = Recency^α × Frequency^β × Importance^γ × ContextualRelevance^δ
//! ```
//!
//! Each memory layer is distinct in type; the `+` in the total is union-of-stores,
//! not a scalar sum.  Relevance scoring drives retrieval priority.

use serde::{Deserialize, Serialize};

use crate::types::{Bounded, Timestep};

/// A single memory record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub layer: MemoryLayer,
    pub content: serde_json::Value,
    /// Absolute timestep when first encoded.
    pub encoded_at: Timestep,
    /// How many times this record has been retrieved.
    pub access_count: u64,
    /// Semantic importance score `[0,1]`.
    pub importance: Bounded,
    /// Contextual tags for relevance matching.
    pub tags: Vec<String>,
}

/// The five memory layers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryLayer {
    /// Very short-lived raw sensory buffer.
    Sensory,
    /// Active working context.
    Working,
    /// Autobiographical episode records.
    Episodic,
    /// General factual / conceptual knowledge.
    Semantic,
    /// Skill and procedure knowledge.
    Procedural,
}

/// Exponents for the relevance scoring formula.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelevanceWeights {
    /// Recency exponent `α`.
    pub alpha: f64,
    /// Frequency exponent `β`.
    pub beta: f64,
    /// Importance exponent `γ`.
    pub gamma: f64,
    /// Contextual relevance exponent `δ`.
    pub delta: f64,
}

impl Default for RelevanceWeights {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            beta: 0.5,
            gamma: 1.5,
            delta: 1.2,
        }
    }
}

/// Score a memory record for retrieval priority.
pub fn relevance_score(
    record: &MemoryRecord,
    current_timestep: Timestep,
    context_match: f64,
    weights: &RelevanceWeights,
) -> f64 {
    let age = (current_timestep - record.encoded_at).max(1) as f64;
    let recency = (1.0 / age).powf(weights.alpha);
    let frequency = (record.access_count as f64 + 1.0).powf(weights.beta);
    let importance = record.importance.value().powf(weights.gamma);
    let contextual = context_match.clamp(0.0, 1.0).powf(weights.delta);
    recency * frequency * importance * contextual
}

/// Multi-layer memory store.
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct MemoryStore {
    records: Vec<MemoryRecord>,
    pub weights: RelevanceWeights,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn encode(&mut self, record: MemoryRecord) {
        self.records.push(record);
    }

    /// Retrieve `n` most relevant records given context match scores.
    pub fn retrieve(
        &self,
        current_timestep: Timestep,
        context_scores: &std::collections::HashMap<String, f64>,
        n: usize,
    ) -> Vec<&MemoryRecord> {
        let mut scored: Vec<(&MemoryRecord, f64)> = self
            .records
            .iter()
            .map(|r| {
                let ctx = r
                    .tags
                    .iter()
                    .filter_map(|t| context_scores.get(t))
                    .cloned()
                    .sum::<f64>()
                    .min(1.0);
                let score =
                    relevance_score(r, current_timestep, ctx, &self.weights);
                (r, score)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scored.into_iter().take(n).map(|(r, _)| r).collect()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}
