//! Worlds to draw in tests, and the one the parity fixture is built from.
//!
//! Public rather than `#[cfg(test)]` on purpose. `web/scripts/check-omni.mjs` asserts that
//! the TypeScript port produces the **same bytes** as this crate for the same world, and
//! that comparison is only meaningful if both sides start from a world nobody can quietly
//! edit on one side. `tests/parity.rs` writes [`parity_world`] and its rendered plate into
//! `fixtures/`, and the check reads them — so Rust's answer is the fixture, exactly as
//! `web/lib/omni/fixtures/record.json` carries digests Rust computed.
//!
//! [`parity_world`] is deliberately awkward. It carries every distinction the plate can
//! draw — a counted signal and an estimated one, both polarities, all four provenances,
//! blind spots, a bounded extent — plus a label containing characters that have to be
//! escaped and a non-ASCII observer. A fixture that only exercises the happy path proves
//! that two implementations agree about nothing in particular.

use std::collections::BTreeMap;

use scema_world::{
    Domain, Entity, EntityKind, Extent, Object, Polarity, Provenance, Scalar, Signal,
    WorldState, WORLD_SCHEMA,
};

fn signal(
    id: &str,
    polarity: Polarity,
    label: &str,
    magnitude: f64,
    measured: bool,
) -> Signal {
    Signal {
        id: id.into(),
        polarity,
        label: label.into(),
        detail: String::new(),
        magnitude,
        measured,
        targets: vec![],
        evidence: vec!["counted in fixture".into()],
    }
}

fn object(id: &str, provenance: Provenance) -> Object {
    Object {
        id: id.into(),
        kind: "file".into(),
        label: id.into(),
        attrs: BTreeMap::new(),
        provenance,
    }
}

fn base() -> WorldState {
    WorldState {
        schema: Some(WORLD_SCHEMA.into()),
        observer: "fixture".into(),
        entity: Entity {
            kind: EntityKind::Repository,
            locator: "/tmp/fixture".into(),
            label: "fixture".into(),
        },
        domain: Domain::Software,
        observed_at: 1_700_000_000,
        objects: vec![],
        facts: vec![],
        signals: vec![],
        extent: Extent::complete(0, "nothing"),
        blind_spots: vec![],
    }
}

/// A world with objects, signals of both polarities, and a bounded extent.
pub fn rich_world() -> WorldState {
    let mut w = base();
    w.entity.label = "scematica".into();
    w.observer = "repo:local".into();
    w.objects = vec![
        object("a", Provenance::Live { age_secs: 1 }),
        object("b", Provenance::Live { age_secs: 4 }),
        object("c", Provenance::Stale { age_secs: 900, budget_secs: 60 }),
        object("d", Provenance::Absent),
    ];
    w.signals = vec![
        signal("s1", Polarity::Risk, "unpinned dependency", 0.62, true),
        signal("s2", Polarity::Opportunity, "tests exist", 0.41, true),
        signal("s3", Polarity::Risk, "TODO markers", 0.18, false),
    ];
    w.extent = Extent::complete(4, "4 objects walked");
    w
}

/// A world whose observer does not know the denominator.
pub fn unbounded_world() -> WorldState {
    let mut w = rich_world();
    w.extent = Extent::partial(4, "depth cap reached");
    w
}

/// A world with nothing in it. Legibility is `0.0` here and in an illegible world, which is
/// the pair the plate has to draw differently.
pub fn empty_world() -> WorldState {
    base()
}

/// The world the Rust and TypeScript renderers are compared on.
///
/// Every branch the plate has, in one world, including the ones that are easy to get wrong:
/// an estimated signal beside a counted one, all four provenance arms so the composition
/// ring has to lay out four segments in a fixed order, a magnitude of exactly `0.0` (a
/// measured zero, which must draw a spoke of zero length rather than no spoke), a magnitude
/// of exactly `1.0`, and a label that must be XML-escaped.
pub fn parity_world() -> WorldState {
    let mut w = base();
    w.entity = Entity {
        kind: EntityKind::Repository,
        locator: "/srv/parity & co".into(),
        label: "parity <fixture> & co".into(),
    };
    w.domain = Domain::Software;
    w.observer = "repo:pärity".into();
    w.observed_at = 1_700_000_000;
    w.objects = vec![
        object("live-1", Provenance::Live { age_secs: 0 }),
        object("live-2", Provenance::Live { age_secs: 12 }),
        object("live-3", Provenance::Live { age_secs: 30 }),
        object("stale-1", Provenance::Stale { age_secs: 7_200, budget_secs: 3_600 }),
        object("absent-1", Provenance::Absent),
        object("sim-1", Provenance::Simulated),
    ];
    w.objects[0]
        .attrs
        .insert("size".into(), Scalar::Int(42));
    w.signals = vec![
        signal("sig-risk-full", Polarity::Risk, "a counted risk at full magnitude", 1.0, true),
        signal("sig-opp-zero", Polarity::Opportunity, "a counted opportunity at zero", 0.0, true),
        signal("sig-risk-est", Polarity::Risk, "an estimated risk", 0.375, false),
        signal("sig-opp-est", Polarity::Opportunity, "an estimated opportunity", 0.5, false),
        signal("sig-opp-odd", Polarity::Opportunity, "an awkward magnitude", 0.333_333_333_3, true),
    ];
    w.extent = Extent {
        observed: 6,
        total: Some(9),
        note: "6 of 9 reached".into(),
    };
    w.blind_spots = vec![
        "could not read /srv/parity/.git".into(),
        "permission denied on /srv/parity/secret".into(),
        "cross-origin frame".into(),
    ];
    w
}
