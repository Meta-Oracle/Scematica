//! Conformance: is this JSON a world omni can safely act on, and if not, exactly why.
//!
//! ## Why this is a module rather than a function in `import`
//!
//! The world contract is a JSON shape implemented in four languages, only one of which can
//! link `scema-world`. Three of the producers are hand-written and no compiler anywhere
//! checks them. Going public multiplies that: anybody can write a producer, in anything,
//! and the only feedback they will ever get is what this module says.
//!
//! So there is exactly one implementation of "is this a usable world", and both callers use
//! it: [`ImportObserver`](crate::ImportObserver) refuses a world that fails, and `scema
//! check` prints the same findings without importing anything. A separate, friendlier
//! checker would be a second brain — it would drift, and the failure mode is the worst one
//! available here: a producer that passes the checker and is then refused by the importer,
//! or the reverse, which teaches an author that the tooling is unreliable and to route
//! around it.
//!
//! ## Every finding, not the first one
//!
//! The previous check bailed on the first violation. That is fine for a guard and hostile
//! as a development loop: a producer author with four problems learns about them one
//! release at a time. [`conform`] returns all of them, ordered, and the importer refuses if
//! any is a [`Level::Fail`].
//!
//! ## What it validates, and what it deliberately does not
//!
//! It validates the **shape** and the internal consistency rules that make downstream output
//! wrong rather than merely odd — a duplicated signal id that `--ground` could not name
//! unambiguously, a magnitude outside `[0,1]` that would dominate a ranking by arithmetic,
//! a signal claiming `measured: true` while citing nothing.
//!
//! It does **not** validate the **claims**. A producer that reports a stale feed as `Live`,
//! or counts a signal it did not count, is lying, and no parser catches that. The honest
//! response is not a deeper check — it is the `imported:` prefix, which tells a reader
//! whose word this is.

use scema_world::{parse_schema, Domain, EntityKind, WorldState, WORLD_SCHEMA, WORLD_SCHEMA_MAJOR};

/// How much a finding matters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// The world is unusable. The importer refuses it.
    Fail,
    /// Legal, and worth a second look. Most often an unfamiliar name, which is either a
    /// deliberate extension or a typo, and this module cannot tell which.
    Warn,
    /// Something the author should know, with nothing to fix.
    Note,
}

impl Level {
    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Fail => "FAIL",
            Level::Warn => "warn",
            Level::Note => "note",
        }
    }
}

/// One thing worth saying about a world.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub level: Level,
    /// Stable identifier, so a producer's own test suite can assert on a rule without
    /// pinning the wording of its message. The messages are meant to be improved.
    pub code: &'static str,
    pub message: String,
    /// What to do about it, when there is a specific thing to do.
    pub fix: Option<String>,
}

impl Finding {
    fn new(level: Level, code: &'static str, message: impl Into<String>) -> Self {
        Finding { level, code, message: message.into(), fix: None }
    }

    fn fix(mut self, f: impl Into<String>) -> Self {
        self.fix = Some(f.into());
        self
    }
}

/// Whether any finding is fatal.
pub fn has_failure(findings: &[Finding]) -> bool {
    findings.iter().any(|f| f.level == Level::Fail)
}

/// Check a world against the contract, reporting everything at once.
pub fn conform(w: &WorldState) -> Vec<Finding> {
    let mut out = Vec::new();
    schema(w, &mut out);
    identity(w, &mut out);
    signals(w, &mut out);
    facts(w, &mut out);
    extent(w, &mut out);
    vocabulary(w, &mut out);
    out
}

/// The version handshake.
///
/// Four distinguishable outcomes, because collapsing them loses the reader's next action.
/// "You are out of date" and "I am out of date" are opposite instructions, and neither is
/// the same as "this was written before the contract had a version at all".
fn schema(w: &WorldState, out: &mut Vec<Finding>) {
    let Some(raw) = w.schema.as_deref() else {
        out.push(
            Finding::new(
                Level::Fail,
                "schema.missing",
                "no `schema` field. A world with no declared contract version cannot be \
                 evolved safely — the next change to the format would be a silent misread \
                 rather than this message.",
            )
            .fix(format!("add `\"schema\": \"{WORLD_SCHEMA}\"` at the top level")),
        );
        return;
    };

    let Some((name, major)) = parse_schema(raw) else {
        out.push(
            Finding::new(
                Level::Fail,
                "schema.malformed",
                format!("`schema` is `{raw}`, which is not a `<name>/<major>` pair"),
            )
            .fix(format!("write `\"schema\": \"{WORLD_SCHEMA}\"`")),
        );
        return;
    };

    if name != "scema.world" {
        out.push(
            Finding::new(
                Level::Fail,
                "schema.foreign",
                format!(
                    "`schema` names the contract `{name}`, not `scema.world`. This is a \
                     well-formed document of some other kind."
                ),
            )
            .fix(format!("if it is a scema world, write `\"schema\": \"{WORLD_SCHEMA}\"`")),
        );
        return;
    }

    match major.cmp(&WORLD_SCHEMA_MAJOR) {
        std::cmp::Ordering::Equal => {
            out.push(Finding::new(Level::Note, "schema.ok", format!("contract {raw}")));
        }
        std::cmp::Ordering::Greater => out.push(
            Finding::new(
                Level::Fail,
                "schema.newer",
                format!(
                    "the world declares `{raw}` and this build reads major \
                     {WORLD_SCHEMA_MAJOR}. The producer is newer than this runtime, so \
                     fields it depends on may be silently ignored here."
                ),
            )
            .fix("upgrade the runtime: `cargo install scema-cli --force`"),
        ),
        std::cmp::Ordering::Less => out.push(
            Finding::new(
                Level::Fail,
                "schema.older",
                format!(
                    "the world declares `{raw}` and this build reads major \
                     {WORLD_SCHEMA_MAJOR}. The producer is written against an older \
                     contract."
                ),
            )
            .fix(format!(
                "update the producer to emit `{WORLD_SCHEMA}`, and check the migration \
                 notes for major {WORLD_SCHEMA_MAJOR}"
            )),
        ),
    }
}

fn identity(w: &WorldState, out: &mut Vec<Finding>) {
    if w.entity.locator.trim().is_empty() {
        out.push(
            Finding::new(
                Level::Fail,
                "entity.no-locator",
                "the entity has no locator; it is what a decision record cites to find this \
                 thing again",
            )
            .fix("set `entity.locator` to a path, URL, PID or address"),
        );
    }
    if w.observer.trim().is_empty() {
        // Not fatal: the importer stamps `imported:unknown`, which is honest. But an
        // unattributable observation is a worse record than an attributed one.
        out.push(
            Finding::new(
                Level::Warn,
                "observer.anonymous",
                "no `observer` name; this will be recorded as `imported:unknown`",
            )
            .fix("set `observer` to the producer's own name, e.g. `my-tool`"),
        );
    }
}

fn signals(w: &WorldState, out: &mut Vec<Finding>) {
    let mut seen: Vec<&str> = Vec::with_capacity(w.signals.len());
    for s in &w.signals {
        if s.id.trim().is_empty() {
            out.push(Finding::new(Level::Fail, "signal.empty-id", "a signal has an empty id"));
        } else if seen.contains(&s.id.as_str()) {
            out.push(
                Finding::new(
                    Level::Fail,
                    "signal.duplicate-id",
                    format!(
                        "two signals share the id `{}`; `--ground` could not name either \
                         unambiguously, and the two branches built from them would rank as \
                         independent support for one thing",
                        s.id
                    ),
                )
                .fix("make every signal id unique within the world"),
            );
        }
        seen.push(s.id.as_str());

        if !s.magnitude.is_finite() || s.magnitude < 0.0 || s.magnitude > 1.0 {
            out.push(
                Finding::new(
                    Level::Fail,
                    "signal.magnitude-range",
                    format!(
                        "signal `{}` has magnitude {}, outside [0,1] — it would dominate a \
                         ranking through arithmetic rather than through importance",
                        s.id, s.magnitude
                    ),
                )
                .fix("normalise the magnitude in the producer, where the scale is known"),
            );
        }

        // The rule this whole runtime exists to enforce, at the one boundary where an
        // outsider's data enters it.
        if s.measured && s.evidence.is_empty() {
            out.push(
                Finding::new(
                    Level::Fail,
                    "signal.uncited-measurement",
                    format!(
                        "signal `{}` claims `measured: true` and cites no evidence. \
                         `scema-sim` scores a real expected gain from a counted signal, so \
                         this is a guess that nothing downstream could tell from a count.",
                        s.id
                    ),
                )
                .fix("either cite what was counted in `evidence`, or set `measured: false`"),
            );
        }
    }

    let counted = w.signals.iter().filter(|s| s.measured).count();
    if !w.signals.is_empty() {
        out.push(Finding::new(
            Level::Note,
            "signal.counts",
            format!("{} signal(s), {counted} counted", w.signals.len()),
        ));
    }
}

fn facts(w: &WorldState, out: &mut Vec<Finding>) {
    for f in &w.facts {
        if !f.confidence.is_finite() || f.confidence < 0.0 || f.confidence > 1.0 {
            out.push(Finding::new(
                Level::Fail,
                "fact.confidence-range",
                format!(
                    "fact `{} {} {}` has confidence {}, outside [0,1]",
                    f.subject, f.predicate, f.object, f.confidence
                ),
            ));
        }
    }
}

fn extent(w: &WorldState, out: &mut Vec<Finding>) {
    if let Some(total) = w.extent.total {
        if w.extent.observed > total {
            out.push(
                Finding::new(
                    Level::Fail,
                    "extent.impossible",
                    format!(
                        "extent claims {} observed of {total} total, which is over 100% \
                         coverage",
                        w.extent.observed
                    ),
                )
                .fix("if the denominator is unknown, `total` must be null — not a smaller number"),
            );
        }
    } else {
        out.push(Finding::new(
            Level::Note,
            "extent.unknown-total",
            "the denominator is unknown, so coverage will read as a count rather than a \
             fraction — the correct way to report a capped or unregistered read",
        ));
    }

    if w.blind_spots.is_empty() {
        // Not a failure. But a producer that never reports one has usually forgotten that
        // an unreadable thing is a blind spot rather than a zero, and that mistake is
        // invisible in the output it does produce.
        out.push(
            Finding::new(
                Level::Note,
                "blind-spots.none",
                "no blind spots reported. If nothing was unreadable this is correct; if \
                 something was skipped because it could not be read, it belongs here.",
            )
            .fix(
                "a deliberate exclusion is not a blind spot — say that in `extent.note` \
                 instead",
            ),
        );
    }
}

/// Names this build does not recognise.
///
/// A warning rather than a failure, and that asymmetry is the point of an open enum: an
/// unfamiliar domain is the format working as designed. It is still worth printing, because
/// an unfamiliar name is also exactly what a typo looks like, and this module cannot tell
/// the two apart — only the author can.
fn vocabulary(w: &WorldState, out: &mut Vec<Finding>) {
    if let Domain::Other(name) = &w.domain {
        if name.is_empty() {
            out.push(
                Finding::new(Level::Fail, "domain.empty", "`domain` is empty")
                    .fix(format!("use one of: {}", Domain::KNOWN.join(", "))),
            );
        } else {
            out.push(
                Finding::new(
                    Level::Warn,
                    "domain.unknown",
                    format!(
                        "`{name}` is not a domain this build knows. That is legal — the \
                         vocabulary is open — but every specialist will decline on it."
                    ),
                )
                .fix(format!("known domains: {}", Domain::KNOWN.join(", "))),
            );
        }
    }

    if let EntityKind::Other(name) = &w.entity.kind {
        if name.is_empty() {
            out.push(
                Finding::new(Level::Fail, "entity.kind-empty", "`entity.kind` is empty")
                    .fix(format!("use one of: {}", EntityKind::KNOWN.join(", "))),
            );
        } else {
            out.push(
                Finding::new(
                    Level::Warn,
                    "entity.kind-unknown",
                    format!("`{name}` is not an entity kind this build knows. That is legal."),
                )
                .fix(format!("known kinds: {}", EntityKind::KNOWN.join(", "))),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_world::{Entity, Extent, Polarity, Signal};

    fn world() -> WorldState {
        WorldState {
            schema: Some(WORLD_SCHEMA.into()),
            observer: "t".into(),
            entity: Entity {
                kind: EntityKind::Repository,
                locator: "/r".into(),
                label: "r".into(),
            },
            domain: Domain::Software,
            observed_at: 0,
            objects: vec![],
            facts: vec![],
            signals: vec![],
            extent: Extent::complete(0, "walked"),
            blind_spots: vec!["one".into()],
        }
    }

    fn signal(id: &str, measured: bool, magnitude: f64, evidence: Vec<String>) -> Signal {
        Signal {
            id: id.into(),
            polarity: Polarity::Risk,
            label: "l".into(),
            detail: String::new(),
            magnitude,
            measured,
            targets: vec![],
            evidence,
        }
    }

    fn codes(w: &WorldState) -> Vec<&'static str> {
        conform(w).into_iter().filter(|f| f.level == Level::Fail).map(|f| f.code).collect()
    }

    #[test]
    fn a_clean_world_has_no_failures() {
        assert!(codes(&world()).is_empty());
        assert!(!has_failure(&conform(&world())));
    }

    #[test]
    fn every_problem_is_reported_at_once_not_one_per_release() {
        // The development-loop property. A producer author with four bugs should learn about
        // four bugs, not discover them one `cargo install` at a time.
        let mut w = world();
        w.schema = None;
        w.entity.locator = "  ".into();
        w.signals = vec![
            signal("a", true, 0.5, vec![]),
            signal("a", false, 9.0, vec!["e".into()]),
        ];
        let found = codes(&w);
        for expected in [
            "schema.missing",
            "entity.no-locator",
            "signal.uncited-measurement",
            "signal.duplicate-id",
            "signal.magnitude-range",
        ] {
            assert!(found.contains(&expected), "missing {expected} in {found:?}");
        }
    }

    #[test]
    fn a_newer_and_an_older_producer_are_told_opposite_things() {
        let mut w = world();
        w.schema = Some("scema.world/99".into());
        assert_eq!(codes(&w), vec!["schema.newer"]);

        w.schema = Some("scema.world/0".into());
        assert_eq!(codes(&w), vec!["schema.older"]);
    }

    #[test]
    fn a_missing_schema_and_a_malformed_one_are_different_findings() {
        let mut w = world();
        w.schema = None;
        assert_eq!(codes(&w), vec!["schema.missing"]);

        w.schema = Some("1".into());
        assert_eq!(codes(&w), vec!["schema.malformed"]);

        w.schema = Some("other.contract/1".into());
        assert_eq!(codes(&w), vec!["schema.foreign"]);
    }

    #[test]
    fn an_unfamiliar_domain_warns_and_does_not_fail() {
        // The whole point of the open vocabulary: a producer for a domain this build has
        // never heard of is valid, and is told that every specialist will decline on it.
        let mut w = world();
        w.domain = Domain::parse("kubernetes");
        assert!(codes(&w).is_empty());
        let warns: Vec<_> = conform(&w).into_iter().filter(|f| f.level == Level::Warn).collect();
        assert_eq!(warns.len(), 1);
        assert_eq!(warns[0].code, "domain.unknown");
    }

    #[test]
    fn an_uncited_measurement_is_fatal_and_an_uncited_estimate_is_not() {
        // The asymmetry the runtime is built on. `measured: false` claims nothing and needs
        // to cite nothing; `measured: true` is a claim somebody counted something.
        let mut w = world();
        w.signals = vec![signal("a", false, 0.5, vec![])];
        assert!(codes(&w).is_empty());

        w.signals = vec![signal("a", true, 0.5, vec![])];
        assert_eq!(codes(&w), vec!["signal.uncited-measurement"]);
    }

    #[test]
    fn every_finding_that_can_be_acted_on_says_how() {
        let mut w = world();
        w.schema = None;
        w.entity.locator = String::new();
        for f in conform(&w).into_iter().filter(|f| f.level == Level::Fail) {
            assert!(f.fix.is_some(), "{} has no fix", f.code);
        }
    }
}
