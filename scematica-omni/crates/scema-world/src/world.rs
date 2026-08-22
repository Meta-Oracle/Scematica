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

use crate::hypothesis::Reversibility;
use crate::provenance::Provenance;

/// The version of the world contract this build reads and writes.
///
/// Producers stamp it into [`WorldState::schema`]. It exists because the contract is a
/// JSON shape implemented in four languages, only one of which can link this crate — so
/// there is no compiler anywhere to catch a producer written against an older reading of
/// it. Without a version, evolving the format degrades into silent misreads; with one, an
/// importer can say which side is out of date and stop.
pub const WORLD_SCHEMA: &str = "scema.world/1";

/// The major of [`WORLD_SCHEMA`]. Worlds agreeing on this are readable by this build.
pub const WORLD_SCHEMA_MAJOR: u32 = 1;

/// Split a schema string into its name and major: `scema.world/1` -> `("scema.world", 1)`.
///
/// `None` for anything not of that shape. The caller decides what to do about it — this
/// crate is pure and does not get to define an import policy.
pub fn parse_schema(s: &str) -> Option<(&str, u32)> {
    let (name, major) = s.trim().rsplit_once('/')?;
    Some((name, major.parse().ok()?))
}

/// Defines an **open** enum: known arms plus `Other(String)`, carried on the wire as a
/// plain lowercase string in both directions.
///
/// Open rather than closed, and that is what makes "universal" more than a claim. The
/// contract is JSON implemented in four languages; a closed enum means that naming a new
/// kind of world — a dataset, a cluster, a document corpus, a spreadsheet — requires a
/// release of `scema-world` and a coordinated upgrade of every producer. Nobody outside
/// this repository can do that, so in practice a closed enum reserves the right to
/// describe reality to this crate's author. `Other` holds an unrecognised name **verbatim**
/// so that it round-trips through a decision record byte for byte, which is what hashing
/// that record requires.
///
/// Parsing normalises case and surrounding whitespace, so `"Software"` and `" software "`
/// are one domain rather than two. It cannot normalise *synonyms*: `k8s` and `kubernetes`
/// are different strings and therefore different domains. That is why the known names are
/// enumerated in `KNOWN` and printed by `scema check` — a producer author should be able to
/// read the vocabulary rather than guess at it.
macro_rules! open_enum {
    (
        $(#[$meta:meta])*
        $name:ident { $( $(#[$vmeta:meta])* $variant:ident => $text:literal ),+ $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )+
            /// A name this build does not know, held verbatim.
            ///
            /// Build it with `parse` rather than directly: `Other("software")` would
            /// serialise as `"software"` and read back as the known arm, so the value would
            /// not survive its own round trip.
            Other(String),
        }

        impl $name {
            /// Every name this build knows, in declaration order.
            pub const KNOWN: &'static [&'static str] = &[ $( $text ),+ ];

            /// The wire spelling.
            pub fn as_str(&self) -> &str {
                match self {
                    $( $name::$variant => $text, )+
                    $name::Other(s) => s.as_str(),
                }
            }

            /// Parse a wire spelling, normalising case and surrounding whitespace.
            ///
            /// Never fails. An unknown name becomes `Other`, because refusing to parse a
            /// name this build has not heard of would make the format extendable only by
            /// whoever ships this crate.
            pub fn parse(s: &str) -> Self {
                let n = s.trim().to_ascii_lowercase();
                match n.as_str() {
                    $( $text => $name::$variant, )+
                    _ => $name::Other(n),
                }
            }

            /// Whether this build knows the name.
            ///
            /// `false` is not an error — it is the reason `Other` exists — but it is worth
            /// surfacing, because it is also what a typo looks like.
            pub fn is_known(&self) -> bool {
                !matches!(self, $name::Other(_))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
                ser.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
                Ok($name::parse(&String::deserialize(de)?))
            }
        }
    };
}

/// The thing being observed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub kind: EntityKind,
    /// How to find it again: a path, a URL, a PID, a mint address.
    pub locator: String,
    /// Human label for rendering.
    pub label: String,
}

open_enum! {
    /// What kind of thing was observed.
    ///
    /// Open — see the `open_enum` macro. The known arms are the ones omni's own producers
    /// emit; a dataset, a cluster, a document corpus or a spreadsheet is `Other` and is
    /// carried through untouched.
    EntityKind {
        Repository => "repository",
        Filesystem => "filesystem",
        Website => "website",
        Process => "process",
        Service => "service",
        Chain => "chain",
        Market => "market",
        Dataset => "dataset",
        Document => "document",
        Cluster => "cluster",
        /// Perceived, but the observer could not classify it.
        ///
        /// Distinct from a missing entity, and distinct from `Other`: `Unknown` says the
        /// observer looked and could not tell, `Other` says it could tell and this build
        /// has not heard the answer before.
        Unknown => "unknown",
    }
}

open_enum! {
    /// The kind of world this is, so that a domain-specific evaluator can *decline* rather
    /// than pretend.
    ///
    /// Open — see the `open_enum` macro. Closing it was the single largest limit on this
    /// runtime being universal: before it opened, a perceived web page and a set of
    /// Chainlink oracle feeds both had to report `Unknown`, which made two entirely
    /// different worlds indistinguishable to every specialist downstream.
    ///
    /// A specialist matches the arm it understands and declines on everything else, so an
    /// unrecognised domain is safe by construction — `Applicability::OutOfDomain` is the
    /// default outcome, not an error.
    Domain {
        Software => "software",
        Infrastructure => "infrastructure",
        Trading => "trading",
        Web => "web",
        Data => "data",
        Document => "document",
        /// The observer did not determine the domain.
        Unknown => "unknown",
    }
}

impl Domain {
    /// What reversing an edit costs in this kind of world.
    ///
    /// A property of the domain, so it lives with the domain rather than in the
    /// hypothesiser that happened to need it first.
    ///
    /// `Software` is `Recoverable` rather than `Trivial`: source under version control is
    /// revertible, but a change something else already depends on is not one `git checkout`
    /// away. `Trading` is `Irreversible`, which corrects a real understatement — a filled
    /// order cannot be unfilled, and this previously answered `Unknown` for the one domain
    /// here where irreversibility is certain. `Infrastructure` is `Costly`: undoable in
    /// principle, at the price of a rebuild and of whatever ran against it meanwhile.
    /// `Data` is `Costly` for the reason a restore is not free. `Web` and `Document` stay
    /// `Unknown` deliberately — editing a page can mean a draft or a publication, and those
    /// are not the same bet.
    ///
    /// An `Other` domain is `Unknown`, and that is an answer rather than a gap. Nobody has
    /// told omni what undoing costs there, so `Unknown` propagates to an *unmeasured*
    /// reversibility term, which shows up in the coverage instead of being smuggled into
    /// the score. An optimistic default here is exactly how an agent talks itself into an
    /// irreversible action.
    pub fn edit_reversibility(&self) -> Reversibility {
        match self {
            Domain::Software => Reversibility::Recoverable,
            Domain::Infrastructure => Reversibility::Costly,
            Domain::Trading => Reversibility::Irreversible,
            Domain::Data => Reversibility::Costly,
            Domain::Web | Domain::Document | Domain::Unknown | Domain::Other(_) => {
                Reversibility::Unknown
            }
        }
    }
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
    /// Which version of the world contract this was written against — [`WORLD_SCHEMA`].
    ///
    /// `None` means the world was produced before the contract carried a version. That is
    /// distinct from an *unrecognised* version, and the importer treats the two
    /// differently: an old record is readable history, an unknown major is a producer this
    /// build cannot safely interpret.
    ///
    /// Omitted from the JSON when absent, which is load-bearing rather than tidy.
    /// `scema-verify` hashes the serialised world, so a field appearing out of nowhere
    /// would change the digest of every record already sealed on disk and report untouched
    /// history as tampered — the one failure that teaches a reader to stop believing the
    /// verifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<String>,
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
            schema: Some(WORLD_SCHEMA.into()),
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

    // ── the open contract ───────────────────────────────────────────────────────────
    //
    // These pin the properties a producer written in another language depends on. Three
    // of the four producers on this contract are hand-written and cannot be type-checked
    // against it, so what is asserted here is the whole of the guarantee they get.

    #[test]
    fn an_unknown_domain_survives_its_own_round_trip_verbatim() {
        // The point of opening the enum: someone observing a Kubernetes cluster can say so,
        // and the name reaches a decision record unchanged. If this dropped to `unknown`,
        // every specialist downstream would be back to being unable to tell one unfamiliar
        // world from another, which is the exact defect opening the enum was meant to fix.
        let d = Domain::parse("kubernetes");
        assert_eq!(d, Domain::Other("kubernetes".into()));
        let json = serde_json::to_string(&d).unwrap();
        assert_eq!(json, "\"kubernetes\"");
        assert_eq!(serde_json::from_str::<Domain>(&json).unwrap(), d);
        assert!(!d.is_known());
    }

    #[test]
    fn parsing_normalises_case_and_padding_but_not_synonyms() {
        assert_eq!(Domain::parse(" Software "), Domain::Software);
        assert_eq!(EntityKind::parse("REPOSITORY"), EntityKind::Repository);
        // Not synonyms, and deliberately not guessed at — `k8s` and `kubernetes` are two
        // domains. The known list is published so a producer author picks rather than
        // invents.
        assert_ne!(Domain::parse("k8s"), Domain::parse("kubernetes"));
    }

    #[test]
    fn a_known_name_never_parses_into_the_other_arm() {
        for name in Domain::KNOWN {
            assert!(Domain::parse(name).is_known(), "{name} fell through to Other");
        }
        for name in EntityKind::KNOWN {
            assert!(EntityKind::parse(name).is_known(), "{name} fell through to Other");
        }
    }

    #[test]
    fn a_world_without_a_schema_serialises_without_the_key() {
        // Load-bearing. `scema-verify` hashes the serialised world, so if this field were
        // written as `null` on a record sealed before the field existed, every such record
        // would report INVALID on a byte nobody touched — and a verifier that cries tamper
        // on untouched history is worse than no verifier.
        let mut w = world(vec![]);
        w.schema = None;
        let json = serde_json::to_string(&w).unwrap();
        assert!(!json.contains("schema"), "absent schema must not be written: {json}");

        w.schema = Some(WORLD_SCHEMA.into());
        assert!(serde_json::to_string(&w).unwrap().contains("scema.world/1"));
    }

    #[test]
    fn an_absent_schema_is_distinct_from_an_unrecognised_one() {
        // Two different claims: "written before this was versioned" and "written by a
        // producer this build cannot interpret". The importer acts differently on each.
        let legacy: WorldState = serde_json::from_str(
            &serde_json::to_string(&{
                let mut w = world(vec![]);
                w.schema = None;
                w
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(legacy.schema, None);

        let future: WorldState = serde_json::from_str(
            &serde_json::to_string(&{
                let mut w = world(vec![]);
                w.schema = Some("scema.world/9".into());
                w
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(parse_schema(future.schema.as_deref().unwrap()), Some(("scema.world", 9)));
    }

    #[test]
    fn schema_strings_that_are_not_the_expected_shape_parse_to_nothing() {
        assert_eq!(parse_schema(WORLD_SCHEMA), Some(("scema.world", WORLD_SCHEMA_MAJOR)));
        assert_eq!(parse_schema("scema.world"), None);
        assert_eq!(parse_schema("scema.world/one"), None);
        assert_eq!(parse_schema(""), None);
    }

    #[test]
    fn a_filled_order_cannot_be_unfilled() {
        // The correction that came with the reversibility table moving onto `Domain`. This
        // previously answered `Unknown` for trading, which is the one domain here where
        // irreversibility is certain rather than undetermined.
        assert_eq!(Domain::Trading.edit_reversibility(), Reversibility::Irreversible);
        assert_eq!(Domain::Software.edit_reversibility(), Reversibility::Recoverable);
    }

    #[test]
    fn an_unfamiliar_domain_gets_unknown_reversibility_not_an_optimistic_guess() {
        // An optimistic default here is how an agent talks itself into an irreversible
        // action. `Unknown` scores as an *unmeasured* term, which shows in the coverage.
        assert_eq!(Domain::parse("kubernetes").edit_reversibility(), Reversibility::Unknown);
        assert_eq!(Domain::parse("kubernetes").edit_reversibility().score(), None);
    }
}
