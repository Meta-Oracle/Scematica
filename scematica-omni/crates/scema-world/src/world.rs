//! [`WorldState`]: what the agent believes is out there, right now, and how it knows.
//!
//! One representation for every environment. A git repository, a web page, a running
//! process and a liquidity pool all reduce to the same shape — entities, objects with
//! attributes, facts, and signals — so that every layer above (simulation, policy,
//! verification, memory) is written once and not per-domain. [`Domain`] exists so that a
//! domain-specific evaluator can *decline* rather than pretend, which is the only reason
//! the type system needs to know the difference at all.
//!
//! Everything here is ordered: attribute maps are [`BTreeMap`] and collections keep the
//! order the observer produced. That is not tidiness — `scema-verify` hashes this
//! structure, and a `HashMap` would produce a different digest for the same world on
//! every run, which would make every decision record unverifiable.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::provenance::Provenance;

/// The thing being observed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub kind: EntityKind,
    /// How to find it again: a path, a URL, a PID, a mint address.
    pub locator: String,
    /// Human label for rendering.
    pub label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntityKind {
    Repository,
    Filesystem,
    Website,
    Process,
    Service,
    Chain,
    Market,
    /// Perceived, but the observer could not classify it. Distinct from a missing entity.
    Unknown,
}

/// The kind of world this is, used only to let a specialist decline politely.
///
/// A trading policy asked to rank a refactor of a filter pipeline has no opinion. The
/// failure mode this prevents is that it produces one anyway — the numbers come out the
/// right shape and nothing downstream can tell they are meaningless.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Domain {
    Software,
    Infrastructure,
    Trading,
    Unknown,
}

/// A scalar attribute value.
///
/// Deliberately small and closed. `serde_json::Value` would allow nested structure, which
/// would then have to be canonicalised for hashing and compared for equality by every
/// consumer; a flat scalar keeps the digest rules in one place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", content = "v", rename_all = "lowercase")]
pub enum Scalar {
    Int(i64),
    Num(f64),
    Text(String),
    Bool(bool),
}

impl Scalar {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Scalar::Int(i) => Some(*i as f64),
            Scalar::Num(n) => Some(*n),
            Scalar::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            Scalar::Text(_) => None,
        }
    }

    pub fn render(&self) -> String {
        match self {
            Scalar::Int(i) => i.to_string(),
            Scalar::Num(n) => format!("{n:.4}"),
            Scalar::Text(s) => s.clone(),
            Scalar::Bool(b) => b.to_string(),
        }
    }
}

/// Something in the world with attributes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Object {
    /// Stable within a world; hypotheses name their targets with it.
    pub id: String,
    /// Free-form class, e.g. `crate`, `file`, `dependency`, `endpoint`, `pool`.
    pub kind: String,
    pub label: String,
    /// Ordered for hashing. See the module note.
    pub attrs: BTreeMap<String, Scalar>,
    pub provenance: Provenance,
}

impl Object {
    pub fn new(
        id: impl Into<String>,
        kind: impl Into<String>,
        label: impl Into<String>,
        provenance: Provenance,
    ) -> Self {
        Object { id: id.into(), kind: kind.into(), label: label.into(), attrs: BTreeMap::new(), provenance }
    }

    pub fn with(mut self, key: impl Into<String>, value: Scalar) -> Self {
        self.attrs.insert(key.into(), value);
        self
    }

    pub fn num(&self, key: &str) -> Option<f64> {
        self.attrs.get(key).and_then(Scalar::as_f64)
    }
}

/// A claim about the world, with confidence and evidence attached.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    /// In `[0, 1]`. A fact nobody could confirm belongs here with empty evidence and low
    /// confidence, not omitted — an agent that silently drops what it half-knows cannot
    /// later explain why it ignored it.
    pub confidence: f64,
    pub evidence: Vec<String>,
    pub provenance: Provenance,
}

/// A risk or an opportunity: something in the world that ought to change a decision.
///
/// The two share a type because they differ only in sign; keeping them apart produced two
/// near-identical structs that drifted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Signal {
    pub id: String,
    pub polarity: Polarity,
    pub label: String,
    pub detail: String,
    /// In `[0, 1]`. Magnitude of the concern or the prize.
    pub magnitude: f64,
    /// Whether `magnitude` was counted or estimated by a rule of thumb. A magnitude the
    /// observer guessed must not move a utility score as if it had been counted.
    pub measured: bool,
    /// Object ids this signal is about. Empty means "the entity as a whole".
    pub targets: Vec<String>,
    pub evidence: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Polarity {
    Risk,
    Opportunity,
}

/// How much of the entity the observer actually reached.
///
/// `total: None` means the observer does not know how many there are, which is a
/// different statement from `total: Some(n)` with `observed == n`. Depth-limited walks,
/// paginated APIs and partially-rendered pages all produce the former.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Extent {
    pub observed: u64,
    pub total: Option<u64>,
    pub note: String,
}

impl Extent {
    pub fn complete(n: u64, note: impl Into<String>) -> Self {
        Extent { observed: n, total: Some(n), note: note.into() }
    }

    pub fn partial(observed: u64, note: impl Into<String>) -> Self {
        Extent { observed, total: None, note: note.into() }
    }

    /// Fraction seen, or `None` when the denominator is unknown. Callers must handle the
    /// `None` rather than defaulting to `1.0`; assuming full coverage is how an agent
    /// becomes confident about a directory it only read the first level of.
    pub fn fraction(&self) -> Option<f64> {
        match self.total {
            Some(0) => Some(1.0),
            Some(t) => Some((self.observed as f64 / t as f64).min(1.0)),
            None => None,
        }
    }
}

/// Everything the agent believes about one entity at one moment.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldState {
    /// Which observer produced this, so a decision record can say what did the looking.
    pub observer: String,
    pub entity: Entity,
    pub domain: Domain,
    /// Unix seconds. Not `chrono` — this crate crosses into the browser extension as JSON
    /// and every dependency it takes is one a reimplementer has to match.
    pub observed_at: i64,
    pub objects: Vec<Object>,
    pub facts: Vec<Fact>,
    pub signals: Vec<Signal>,
    pub extent: Extent,
    /// Things the observer tried to read and could not, verbatim. This is not a log — it
    /// is an input: `scema-sim` raises uncertainty from it, and an agent that discards it
    /// cannot tell a clean world from a world it failed to look at.
    pub blind_spots: Vec<String>,
}

impl WorldState {
    pub fn object(&self, id: &str) -> Option<&Object> {
        self.objects.iter().find(|o| o.id == id)
    }

    pub fn risks(&self) -> impl Iterator<Item = &Signal> {
        self.signals.iter().filter(|s| s.polarity == Polarity::Risk)
    }

    pub fn opportunities(&self) -> impl Iterator<Item = &Signal> {
        self.signals.iter().filter(|s| s.polarity == Polarity::Opportunity)
    }

    /// Objects whose values may be acted on. Anything `Stale`, `Absent` or `Simulated` is
    /// excluded — see [`Provenance::is_actionable`].
    pub fn actionable_objects(&self) -> impl Iterator<Item = &Object> {
        self.objects.iter().filter(|o| o.provenance.is_actionable())
    }

    /// The share of observed objects that are actionable, in `[0, 1]`.
    ///
    /// An empty world scores `0.0`, not `1.0`. "Nothing was unreadable" and "there was
    /// nothing to read" must not produce the same number.
    pub fn legibility(&self) -> f64 {
        if self.objects.is_empty() {
            return 0.0;
        }
        self.actionable_objects().count() as f64 / self.objects.len() as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world(objects: Vec<Object>) -> WorldState {
        WorldState {
            observer: "test".into(),
            entity: Entity { kind: EntityKind::Repository, locator: ".".into(), label: "t".into() },
            domain: Domain::Software,
            observed_at: 0,
            objects,
            facts: vec![],
            signals: vec![],
            extent: Extent::partial(0, "test"),
            blind_spots: vec![],
        }
    }

    #[test]
    fn an_empty_world_is_illegible_not_perfectly_legible() {
        assert_eq!(world(vec![]).legibility(), 0.0);
    }

    #[test]
    fn stale_objects_do_not_count_as_legible() {
        let w = world(vec![
            Object::new("a", "file", "a", Provenance::Live { age_secs: 1 }),
            Object::new("b", "file", "b", Provenance::Stale { age_secs: 99, budget_secs: 10 }),
        ]);
        assert_eq!(w.legibility(), 0.5);
    }

    #[test]
    fn unknown_extent_is_not_full_coverage() {
        assert_eq!(Extent::partial(12, "depth limited").fraction(), None);
        assert_eq!(Extent::complete(12, "walked").fraction(), Some(1.0));
    }
}
