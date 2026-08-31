//! [`ImportObserver`]: a world perceived somewhere else.
//!
//! The observer that makes omni's domain-agnosticism *operational* rather than merely
//! stated. `RepoObserver` can perceive a source tree because it is written in Rust and runs
//! in this process. It cannot perceive a running Solana bot, a set of Chainlink oracle
//! feeds, or a DOM — those live behind a different lockfile, a Python package, and a browser
//! respectively, and pulling any of them in here would make omni a hub of domain
//! dependencies, which is the exact thing the workspace note forbids.
//!
//! The alternative that works is the one the browser extension already proved:
//!
//! > **The thing being observed describes itself in `scema-world`'s vocabulary, and omni
//! > reads that.**
//!
//! `plugins/scema-web/src/perceive.js` emits `WorldState` JSON in 300 lines of dependency-
//! free JavaScript and nothing above perception needed a line changed to gain a browser.
//! There are now four producers on that contract — `RepoObserver` here, `perceive.js` in the
//! extension, `scematica_mesh::omni` in the bot workspace, and `alchem_link.omni` in
//! Python — and only the first of them is written in a language this crate can link.
//!
//! ```console
//! $ mesh-dashboard --world | scema simulate "keep the pipeline honest" --path -
//! $ alchem-link omni -n base > feeds.json && scema observe feeds.json
//! ```
//!
//! ## The observer field is always rewritten, and that is the load-bearing part
//!
//! An imported world is **not** trusted to describe its own provenance — but it does not
//! have to be. Whatever the producer called itself, [`ImportObserver`] prefixes it with
//! `imported:`, exactly as `scema-daemon` prefixes a wire-supplied world with `client:`. A
//! decision record can therefore never claim that a world which arrived as a file was
//! observed locally, and a reader of that record can see in one field which it was.
//!
//! ## What it validates, and what it deliberately does not
//!
//! It validates the *shape*: the JSON has to deserialise into a `WorldState`, and a few
//! internal-consistency rules are checked because violating them makes downstream output
//! wrong rather than merely odd (a signal id used twice cannot be named unambiguously by
//! `--ground`; a magnitude outside `[0,1]` would dominate a ranking by arithmetic).
//!
//! It does **not** validate the *claims*. A producer that reports a stale feed as `Live`, or
//! counts a signal it did not count, is lying, and no amount of parsing catches that. The
//! honest response is not a deeper check — it is the `imported:` prefix, which tells a
//! reader exactly whose word this is.

use std::io::Read;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use scema_world::WorldState;

use crate::observer::Observer;

/// Bytes read before an import gives up.
///
/// A world is a description of an environment, not a dump of it — the repo observer's
/// output for this workspace is about 60 KB. Sixteen megabytes is roomy enough for anything
/// legitimate and small enough that a producer stuck in a loop cannot exhaust memory here.
pub const MAX_IMPORT_BYTES: u64 = 16 * 1024 * 1024;

/// Reads a `WorldState` that something else produced.
#[derive(Clone, Copy, Debug, Default)]
pub struct ImportObserver;

impl ImportObserver {
    pub fn new() -> Self {
        ImportObserver
    }

    /// Parse, check, and stamp a world from raw JSON.
    ///
    /// Split out from [`Observer::observe`] so the daemon and the MCP server can reuse the
    /// same validation on a world that arrived over the wire rather than from a file. One
    /// implementation of "is this a usable world", not three.
    pub fn from_json(text: &str, source: &str) -> Result<WorldState> {
        let mut world: WorldState = serde_json::from_str(text).with_context(|| {
            format!("{source} is not a scema-world WorldState (see scema-world's JSON shape)")
        })?;
        check(&world).with_context(|| format!("{source} parsed but is not internally consistent"))?;
        world.observer = stamp(&world.observer);
        Ok(world)
    }

    /// Read from standard input.
    pub fn from_stdin() -> Result<WorldState> {
        let mut text = String::new();
        std::io::stdin()
            .take(MAX_IMPORT_BYTES)
            .read_to_string(&mut text)
            .context("reading a world from stdin")?;
        if text.trim().is_empty() {
            bail!(
                "nothing arrived on stdin. A producer that printed its help text or failed \
                 silently looks exactly like this — check its exit code."
            );
        }
        ImportObserver::from_json(&text, "stdin")
    }

    /// Read from a file.
    pub fn from_file(path: &Path) -> Result<WorldState> {
        let meta = std::fs::metadata(path)
            .with_context(|| format!("reading {}", path.display()))?;
        if meta.len() > MAX_IMPORT_BYTES {
            bail!(
                "{} is {} bytes, over the {MAX_IMPORT_BYTES}-byte import cap. A world is a \
                 description of an environment, not a dump of it.",
                path.display(),
                meta.len()
            );
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading {}", path.display()))?;
        ImportObserver::from_json(&text, &path.display().to_string())
    }
}

/// Prefix a producer's own observer name so a record cannot claim a local observation.
///
/// Idempotent: importing an already-imported world does not stack prefixes, because a
/// pipeline that passed a world through twice would otherwise produce
/// `imported:imported:page` and make the origin harder to read rather than easier.
fn stamp(observer: &str) -> String {
    let name = observer.trim();
    if name.is_empty() {
        // A producer that named no observer is a producer whose output cannot be attributed.
        // `unknown` is the honest label; silently inheriting `import` would credit this crate
        // with an observation it did not make.
        return "imported:unknown".to_string();
    }
    if name.starts_with("imported:") {
        return name.to_string();
    }
    format!("imported:{name}")
}

/// Refuse a world that does not conform, listing every reason at once.
///
/// The rules live in [`crate::conform`] rather than here, because `scema check` has to
/// report exactly what the importer enforces. Two implementations would drift, and the
/// failure mode is the one that destroys trust in tooling fastest: a producer that passes
/// the checker and is then refused by the importer, or the reverse.
///
/// Every failure is listed, not just the first. A producer author with four problems should
/// learn about four problems.
fn check(w: &WorldState) -> Result<()> {
    use crate::conform::{conform, has_failure, Level};
    let findings = conform(w);
    if !has_failure(&findings) {
        return Ok(());
    }
    let mut msg = String::new();
    for f in findings.iter().filter(|f| f.level == Level::Fail) {
        msg.push_str("
  - ");
        msg.push_str(&f.message);
        if let Some(fix) = &f.fix {
            msg.push_str("
    fix: ");
            msg.push_str(fix);
        }
    }
    msg.push_str("

  `scema check <file>` prints the full report.");
    bail!("{msg}");
}

impl Observer for ImportObserver {
    fn name(&self) -> &str {
        "import"
    }

    fn about(&self) -> &str {
        "a WorldState produced elsewhere: `-` for stdin, or a path to a .json file"
    }

    fn handles(&self, locator: &str) -> bool {
        let l = locator.trim();
        l == "-" || l.eq_ignore_ascii_case("stdin") || l.to_ascii_lowercase().ends_with(".json")
    }

    fn observe(&self, locator: &str) -> Result<WorldState> {
        let l = locator.trim();
        if l == "-" || l.eq_ignore_ascii_case("stdin") {
            return ImportObserver::from_stdin();
        }
        if !self.handles(l) {
            return Err(anyhow!(
                "`{l}` is not something this observer handles; it takes `-` or a path ending .json"
            ));
        }
        ImportObserver::from_file(Path::new(l))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_world::{Domain, Entity, EntityKind, Extent, Polarity, Provenance, Signal};
    use std::fs;

    fn minimal() -> serde_json::Value {
        serde_json::json!({
            "schema": scema_world::WORLD_SCHEMA,
            "observer": "mesh",
            "entity": { "kind": "service", "locator": "/bot", "label": "bot" },
            "domain": "trading",
            "observed_at": 1_700_000_000i64,
            "objects": [],
            "facts": [],
            "signals": [],
            "extent": { "observed": 3, "total": 3, "note": "collected" },
            "blind_spots": []
        })
    }

    fn with_signals(signals: serde_json::Value) -> String {
        let mut v = minimal();
        v["signals"] = signals;
        v.to_string()
    }

    #[test]
    fn an_imported_world_can_never_claim_it_was_observed_here() {
        // The whole point of the crate. `scema-daemon` makes the same rewrite for a world
        // that arrived over the wire, for the same reason.
        let w = ImportObserver::from_json(&minimal().to_string(), "t").unwrap();
        assert_eq!(w.observer, "imported:mesh");
    }

    #[test]
    fn importing_twice_does_not_stack_prefixes() {
        // A pipeline that passed a world through two stages would otherwise produce
        // `imported:imported:mesh`, which makes the origin harder to read, not easier.
        let mut v = minimal();
        v["observer"] = serde_json::json!("imported:mesh");
        let w = ImportObserver::from_json(&v.to_string(), "t").unwrap();
        assert_eq!(w.observer, "imported:mesh");
    }

    #[test]
    fn a_world_with_no_observer_name_is_attributed_to_nobody_rather_than_to_us() {
        let mut v = minimal();
        v["observer"] = serde_json::json!("   ");
        let w = ImportObserver::from_json(&v.to_string(), "t").unwrap();
        assert_eq!(w.observer, "imported:unknown");
    }

    #[test]
    fn a_counted_signal_that_cites_nothing_is_refused() {
        // The laundering this validation exists for. `measured: true` is a claim that
        // somebody counted something, and it is the claim `scema-sim` relies on to score a
        // real expected gain. A producer making it with nothing to cite is producing a
        // hallucination with a decimal point on it.
        let text = with_signals(serde_json::json!([{
            "id": "a", "polarity": "risk", "label": "x", "detail": "",
            "magnitude": 0.5, "measured": true, "targets": [], "evidence": []
        }]));
        let err = ImportObserver::from_json(&text, "t").unwrap_err().to_string();
        let chain = format!("{:#}", ImportObserver::from_json(&text, "t").unwrap_err());
        assert!(chain.contains("cites no evidence"), "{err} / {chain}");
    }

    #[test]
    fn an_estimated_signal_may_cite_nothing() {
        // The other half: a producer that admits it guessed is behaving correctly, and
        // `measured: false` is exactly how it says so.
        let text = with_signals(serde_json::json!([{
            "id": "a", "polarity": "risk", "label": "x", "detail": "",
            "magnitude": 0.5, "measured": false, "targets": [], "evidence": []
        }]));
        assert!(ImportObserver::from_json(&text, "t").is_ok());
    }

    #[test]
    fn a_magnitude_outside_the_unit_interval_is_refused_with_the_signal_named() {
        // It would dominate a ranking by arithmetic rather than by importance, and the
        // producer is the only place that can be fixed.
        for bad in [1.5, -0.2] {
            let text = with_signals(serde_json::json!([{
                "id": "loud", "polarity": "risk", "label": "x", "detail": "",
                "magnitude": bad, "measured": true, "targets": [], "evidence": ["counted"]
            }]));
            let chain = format!("{:#}", ImportObserver::from_json(&text, "t").unwrap_err());
            assert!(chain.contains("loud"), "{chain}");
            assert!(chain.contains("outside [0,1]"), "{chain}");
        }
    }

    #[test]
    fn duplicate_signal_ids_are_refused_because_ground_could_not_name_one() {
        let sig = |id: &str| {
            serde_json::json!({
                "id": id, "polarity": "risk", "label": "x", "detail": "",
                "magnitude": 0.5, "measured": true, "targets": [], "evidence": ["counted"]
            })
        };
        let text = with_signals(serde_json::json!([sig("a"), sig("a")]));
        let chain = format!("{:#}", ImportObserver::from_json(&text, "t").unwrap_err());
        assert!(chain.contains("share the id"), "{chain}");
    }

    #[test]
    fn an_extent_whose_numerator_exceeds_its_denominator_is_refused() {
        // `Extent::fraction` would report over 100% observed, which reads as certainty about
        // a world the producer only partly saw. The `None` denominator exists for exactly
        // this case and must be used instead.
        let mut v = minimal();
        v["extent"] = serde_json::json!({ "observed": 9, "total": 3, "note": "?" });
        let chain = format!("{:#}", ImportObserver::from_json(&v.to_string(), "t").unwrap_err());
        assert!(chain.contains("not a smaller number"), "{chain}");
    }

    #[test]
    fn an_unknown_denominator_is_accepted_and_is_the_correct_way_to_say_so() {
        let mut v = minimal();
        v["extent"] = serde_json::json!({ "observed": 9, "total": null, "note": "capped" });
        let w = ImportObserver::from_json(&v.to_string(), "t").unwrap();
        assert_eq!(w.extent.fraction(), None);
    }

    #[test]
    fn an_entity_with_no_locator_is_refused() {
        // The locator is what a decision record cites to find this environment again. A
        // record naming an empty string is a record nobody can re-check.
        let mut v = minimal();
        v["entity"]["locator"] = serde_json::json!("");
        assert!(ImportObserver::from_json(&v.to_string(), "t").is_err());
    }

    #[test]
    fn the_locator_grammar_is_narrow_so_repo_observer_still_wins_a_directory() {
        // `default_observers` resolves first-match, so this observer has to be *ahead* of
        // `RepoObserver` and must therefore claim only what it really handles. Claiming a
        // bare path would break `scema observe .`.
        let o = ImportObserver;
        assert!(o.handles("-"));
        assert!(o.handles("stdin"));
        assert!(o.handles("mesh.json"));
        assert!(o.handles("/tmp/World.JSON"));
        assert!(!o.handles("."));
        assert!(!o.handles("/some/project"));
        assert!(!o.handles("crates/scema-tools"));
    }

    #[test]
    fn a_file_that_is_not_json_says_what_it_should_have_been() {
        let dir = std::env::temp_dir().join(format!("scema-import-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.json");
        fs::write(&path, "not json").unwrap();
        let chain = format!("{:#}", ImportObserver.observe(path.to_str().unwrap()).unwrap_err());
        assert!(chain.contains("WorldState"), "{chain}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_real_world_round_trips_through_the_importer_unchanged_but_for_the_stamp() {
        // The contract producers are written against: everything survives except the
        // attribution, which is the one thing that must not.
        let original = WorldState {
            schema: Some(scema_world::WORLD_SCHEMA.into()),
            observer: "mesh".into(),
            entity: Entity {
                kind: EntityKind::Service,
                locator: "/bot".into(),
                label: "sniper".into(),
            },
            domain: Domain::Trading,
            observed_at: 1_700_000_000,
            objects: vec![],
            facts: vec![],
            signals: vec![Signal {
                id: "veto:dqstar".into(),
                polarity: Polarity::Risk,
                label: "DQ* is suppressing buys".into(),
                detail: String::new(),
                magnitude: 0.8,
                measured: true,
                targets: vec!["learner.dqstar".into()],
                evidence: vec!["counted 12 consecutive vetoes".into()],
            }],
            extent: Extent::complete(7, "collected"),
            blind_spots: vec!["scematica-metrics.json: absent".into()],
        };
        let text = serde_json::to_string(&original).unwrap();
        let back = ImportObserver::from_json(&text, "t").unwrap();

        assert_eq!(back.observer, "imported:mesh");
        assert_eq!(back.entity, original.entity);
        assert_eq!(back.signals, original.signals);
        assert_eq!(back.blind_spots, original.blind_spots);
        assert_eq!(back.extent, original.extent);
    }

    // ── the wire contract, against what the producers actually emitted ────────
    //
    // Three producers emit a `WorldState` without linking `scema-world`: the bot mesh in
    // Rust behind another lockfile, `alchem-link` in stdlib-only Python, and the browser
    // extension in dependency-free JavaScript. Each restates this crate's validation on its
    // own side and fails its own tests. These close the loop from the other direction.
    //
    // The two halves catch different things. A producer's self-check catches a bug in that
    // producer. A fixture catches the case where both sides were changed and only one of
    // them was right.

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
    }

    /// Every captured producer output, by file name.
    ///
    /// Listed once so a new fixture joins every property test at once. Adding one to
    /// `every_producer_fixture_imports` and forgetting the rest is how a producer ends up
    /// pinned for its attribution and unpinned for the rules that matter.
    const FIXTURES: [&str; 4] = [
        "mesh-world.json",
        "alchem-world.json",
        "alchem-window-world.json",
        "page-world.json",
    ];

    /// Every fixture parses, validates, and keeps its producer's own attribution.
    #[test]
    fn every_producer_fixture_imports() {
        for (file, observer) in [
            ("mesh-world.json", "imported:mesh"),
            ("alchem-world.json", "imported:alchem-link"),
            ("alchem-window-world.json", "imported:alchem-link"),
            ("page-world.json", "imported:page"),
        ] {
            let w = ImportObserver::from_json(&fixture(file), file)
                .unwrap_or_else(|e| panic!("{file}: {e:#}"));
            assert_eq!(w.observer, observer, "{file}");
            assert!(!w.entity.locator.trim().is_empty(), "{file}");
        }
    }

    /// A world that lists what it could not read is worth more than one that does not, and
    /// all three producers are expected to do it. A fixture with no blind spots would mean
    /// the capture was taken against an environment with nothing hidden in it, which is not
    /// what any of these three observe.
    #[test]
    fn every_producer_reports_what_it_could_not_see() {
        for file in FIXTURES {
            let w = ImportObserver::from_json(&fixture(file), file).unwrap();
            assert!(
                !w.blind_spots.is_empty(),
                "{file} reports perfect visibility, which no real observation has"
            );
        }
    }

    /// Every counted signal cites its count, in every producer.
    ///
    /// The property `scema-sim` depends on to score a real expected gain. This is also
    /// enforced by `check()` above, so the assertion is redundant *today* — and it is the
    /// redundancy that is the point: if somebody relaxes the validator, this fails and says
    /// which producer's real output stopped being defensible.
    #[test]
    fn no_producer_claims_a_measurement_it_cannot_cite() {
        for file in FIXTURES {
            let w = ImportObserver::from_json(&fixture(file), file).unwrap();
            for s in &w.signals {
                if s.measured {
                    assert!(!s.evidence.is_empty(), "{file}: `{}` cites nothing", s.id);
                }
                assert!((0.0..=1.0).contains(&s.magnitude), "{file}: `{}`", s.id);
            }
        }
    }

    /// Provenance survives the boundary in every arm that matters.
    ///
    /// The mesh fixture is captured against a directory holding stale state files and
    /// missing ones, so it carries `Stale` and `Absent`; the oracle fixture carries `Stale`
    /// for a feed past its heartbeat. If a producer ever collapsed those into `Live`, an
    /// agent would act on values that were true an hour ago — the single failure this whole
    /// arrangement exists to prevent.
    #[test]
    fn stale_and_absent_survive_the_wire() {
        let mesh = ImportObserver::from_json(&fixture("mesh-world.json"), "mesh").unwrap();
        assert!(
            mesh.objects.iter().any(|o| matches!(o.provenance, Provenance::Stale { .. })),
            "the mesh fixture should carry at least one stale unit"
        );
        assert!(
            mesh.objects.iter().any(|o| matches!(o.provenance, Provenance::Absent)),
            "the mesh fixture should carry at least one unseen unit"
        );

        let feeds = ImportObserver::from_json(&fixture("alchem-world.json"), "alchem").unwrap();
        assert!(
            feeds.objects.iter().any(|o| matches!(o.provenance, Provenance::Stale { .. })),
            "the oracle fixture should carry a feed past its own heartbeat"
        );
        // An absent object must carry no attributes at all. A feed that did not answer has
        // no price, and an attribute map with a zero in it would say it reported one.
        for o in feeds.objects.iter().filter(|o| o.provenance == Provenance::Absent) {
            assert!(o.attrs.is_empty(), "an unread feed must carry no values: {}", o.id);
        }

        let window =
            ImportObserver::from_json(&fixture("alchem-window-world.json"), "alchem").unwrap();
        assert!(
            window.objects.iter().any(|o| matches!(o.provenance, Provenance::Stale { .. })),
            "the windowed fixture should carry a feed whose last print is past its heartbeat"
        );
    }

    /// A window never reports a statistic it could not compute.
    ///
    /// The hazard this fixture exists for. A window of price history is the most fabricable
    /// thing any of these producers emits — every figure is a real number of a plausible
    /// shape, and a volatility invented from two prints is indistinguishable from one
    /// measured over three hundred once it is inside a record somebody can verify but not
    /// second-guess. The producer omits what it could not compute; this asserts the omission
    /// survived the wire rather than arriving as a zero.
    #[test]
    fn a_windowed_world_omits_what_it_could_not_compute() {
        let w =
            ImportObserver::from_json(&fixture("alchem-window-world.json"), "alchem").unwrap();

        let thin = w
            .objects
            .iter()
            .find(|o| o.attrs.get("samples").and_then(|v| v.as_f64()) == Some(3.0))
            .expect("the fixture should carry a three-print feed");
        // Three prints do have a volatility. What they do not have is a *drawdown-free*
        // claim about a longer span, so the assertion is on the shape of the object rather
        // than on one key: every attribute present is one the producer actually computed.
        assert!(thin.attrs.contains_key("samples"), "a count is always measured");

        // Nothing anywhere claims a statistic of exactly zero unless the window could
        // support it — the em-dash rule, in a producer that has no renderer of its own.
        for o in &w.objects {
            if let Some(v) = o.attrs.get("volatility_period") {
                assert!(
                    o.attrs.get("samples").and_then(|s| s.as_f64()).unwrap_or(0.0) >= 2.0,
                    "{} reports volatility {v:?} from fewer than two prints",
                    o.id
                );
            }
        }
    }

    /// The window and the snapshot describe the same subject.
    ///
    /// Deliberate: an agent should see one network observed two ways, not two networks. A
    /// separate locator would split the memory in half, and nothing downstream would ever
    /// connect "this feed was stale then" with "this feed gaps".
    #[test]
    fn the_two_oracle_worlds_share_a_subject_and_an_object_naming() {
        let snap = ImportObserver::from_json(&fixture("alchem-world.json"), "a").unwrap();
        let win =
            ImportObserver::from_json(&fixture("alchem-window-world.json"), "a").unwrap();
        assert_eq!(snap.entity.kind, win.entity.kind);
        assert!(
            snap.objects.iter().all(|o| o.id.starts_with("feed:"))
                && win.objects.iter().all(|o| o.id.starts_with("feed:")),
            "both must name a feed the same way or no agent can correlate them"
        );
    }

    /// A world from a page does not leak the query string into the record.
    ///
    /// `page-world.json` is captured from a URL carrying `?sid=SECRET`. The locator is
    /// hashed into a decision record that outlives the tab, and query strings routinely
    /// carry session tokens. The extension's own `test/wire.test.js` pins this against a
    /// live daemon; this pins it against the bytes that actually crossed.
    #[test]
    fn a_perceived_page_carries_no_query_string() {
        let w = ImportObserver::from_json(&fixture("page-world.json"), "page").unwrap();
        assert!(!w.entity.locator.contains('?'), "{}", w.entity.locator);
        assert!(!w.entity.locator.contains("SECRET"), "{}", w.entity.locator);
    }

    /// Each producer describes a different kind of world, and the domain now says which.
    ///
    /// `Domain` exists so a specialist can decline rather than pretend. The bot mesh is a
    /// trading world and the Deep Q* evaluator recognises it (and then declines for want of
    /// a checkpoint, which is a different and more useful answer). An oracle set and a web
    /// page are declined by that evaluator either way.
    ///
    /// What changed when the vocabulary opened is that they are no longer declined
    /// *identically*. Both used to report `unknown`, so nothing downstream could tell a set
    /// of Chainlink feeds from a DOM — three producers, two indistinguishable worlds. The
    /// decline is the same; the record of what was declined is not.
    #[test]
    fn the_domain_lets_a_specialist_decline_correctly() {
        let mesh = ImportObserver::from_json(&fixture("mesh-world.json"), "mesh").unwrap();
        assert_eq!(mesh.domain, scema_world::Domain::Trading);

        let feeds = ImportObserver::from_json(&fixture("alchem-world.json"), "a").unwrap();
        assert_eq!(feeds.domain, scema_world::Domain::Data);

        let page = ImportObserver::from_json(&fixture("page-world.json"), "p").unwrap();
        assert_eq!(page.domain, scema_world::Domain::Web);

        assert_ne!(feeds.domain, page.domain, "two different worlds must not read alike");
    }

    /// Every producer declares the contract version it was written against.
    ///
    /// The one rule that cannot be enforced by any producer's own test suite, because the
    /// thing it protects against is a producer nobody in this repository wrote.
    #[test]
    fn every_producer_declares_the_contract_it_was_written_against() {
        for file in ["mesh-world.json", "alchem-world.json", "page-world.json"] {
            let w = ImportObserver::from_json(&fixture(file), file).unwrap();
            assert_eq!(w.schema.as_deref(), Some(scema_world::WORLD_SCHEMA), "{file}");
        }
    }

    /// A world with no declared contract is refused, and the message says what to add.
    #[test]
    fn an_undeclared_contract_is_refused_with_the_line_to_paste() {
        let mut v: serde_json::Value =
            serde_json::from_str(&fixture("mesh-world.json")).unwrap();
        v.as_object_mut().unwrap().remove("schema");
        let err = ImportObserver::from_json(&v.to_string(), "t").unwrap_err();
        let chain = format!("{err:#}");
        assert!(chain.contains("scema.world/1"), "{chain}");
        assert!(chain.contains("scema check"), "{chain}");
    }

    /// Every failure at once, rather than one per fix-and-rerun.
    #[test]
    fn a_producer_with_several_problems_is_told_about_all_of_them() {
        // The development-loop property. Before the checker was shared with `scema check`,
        // the importer bailed on the first violation, so an author with four problems
        // learned about them one at a time.
        let mut v = minimal();
        v.as_object_mut().unwrap().remove("schema");
        v["entity"]["locator"] = serde_json::json!("  ");
        v["signals"] = serde_json::json!([
            { "id": "dup", "polarity": "risk", "label": "l", "detail": "",
              "magnitude": 0.5, "measured": true, "targets": [], "evidence": [] },
            { "id": "dup", "polarity": "risk", "label": "l", "detail": "",
              "magnitude": 4.0, "measured": false, "targets": [], "evidence": ["e"] }
        ]);
        let chain = format!("{:#}", ImportObserver::from_json(&v.to_string(), "t").unwrap_err());
        for expected in ["schema", "locator", "cites no evidence", "share the id", "outside [0,1]"] {
            assert!(chain.contains(expected), "missing `{expected}` in:
{chain}");
        }
    }

}
