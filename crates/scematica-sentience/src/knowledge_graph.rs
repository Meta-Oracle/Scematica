//! §8  Knowledge Graph
//!
//! ```text
//! G = (V, E, W)
//! N_i = (data, source, time, confidence, context)
//! E_ij = (type, strength, confidence, provenance)
//! ```
//!
//! New evidence **modifies** the graph rather than blindly replacing prior information.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::types::{Confidence, Provenance, Timestep};

/// A knowledge node `N_i`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeNode {
    pub id: String,
    pub data: serde_json::Value,
    pub source: String,
    pub created_at: Timestep,
    pub updated_at: Timestep,
    pub confidence: Confidence,
    pub context: String,
}

/// Relationship type between knowledge nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RelationType {
    Causes,
    Supports,
    Contradicts,
    Correlates,
    Implies,
    IsA,
    PartOf,
    Custom(String),
}

/// A directed knowledge edge `E_ij`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEdge {
    pub from: String,
    pub to: String,
    pub relation: RelationType,
    /// Strength of the relationship `[0,1]`.
    pub strength: f64,
    pub confidence: Confidence,
    pub provenance: Provenance,
}

/// The full knowledge graph `G = (V, E, W)`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct KnowledgeGraph {
    nodes: HashMap<String, KnowledgeNode>,
    edges: Vec<KnowledgeEdge>,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add or update a node.  New evidence modifies; it does not blindly overwrite.
    pub fn upsert_node(
        &mut self,
        node: KnowledgeNode,
        current_timestep: Timestep,
    ) {
        self.nodes
            .entry(node.id.clone())
            .and_modify(|existing| {
                // Bayesian-style confidence update: blend existing and new.
                let blended = (existing.confidence.value() + node.confidence.value()) / 2.0;
                existing.confidence = blended.into();
                existing.updated_at = current_timestep;
                // Only overwrite data if confidence improved.
                if node.confidence > existing.confidence {
                    existing.data = node.data.clone();
                }
            })
            .or_insert(node);
    }

    /// Add a directed edge between two nodes.
    pub fn add_edge(&mut self, edge: KnowledgeEdge) {
        self.edges.push(edge);
    }

    pub fn node(&self, id: &str) -> Option<&KnowledgeNode> {
        self.nodes.get(id)
    }

    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// All edges from a given node.
    pub fn edges_from(&self, id: &str) -> impl Iterator<Item = &KnowledgeEdge> {
        self.edges.iter().filter(move |e| e.from == id)
    }
}
