//! The mesh, described in Scematica Omni's vocabulary.
//!
//! `scematica-mesh` answers *what is the running system doing, and can each part of it be
//! seen at all*. Omni answers *given a world, which of these branches is worth taking*.
//! Those are complementary questions and until now nothing joined them: the mesh could tell
//! an operator that three breakers were armed and the DQ* was vetoing, and the operator had
//! to be the one who decided what to do about it.
//!
//! This module emits a [`scema_world::WorldState`] — as JSON, by hand — so that a live bot
//! becomes an environment omni can reason over:
//!
//! ```console
//! $ mesh-dashboard --world | scema simulate "get the pipeline trading again" --path -
//! ```
//!
//! ## Why hand-built JSON rather than a dependency on `scema-world`
//!
//! Because the wire format is the contract, and there are already three other producers on
//! it that could not take the dependency either. `plugins/scema-web/src/perceive.js` emits
//! the same shape in dependency-free JavaScript; `alchem_link/omni.py` emits it from
//! stdlib Python. Omni's own workspace note says nothing there may depend on
//! `scematica-core` or anything downstream — and the inverse coupling, `scematica-mesh`
//! reaching into omni's crates, would put a second lockfile's crate on the bot's dependency
//! graph to save a hundred lines of `json!`.
//!
//! The guarantee that keeps this honest is not a type: it is
//! [`tests::a_mesh_world_is_accepted_by_the_shape_the_importer_enforces`] plus omni's own
//! `ImportObserver`, which refuses a world that is malformed *or* internally inconsistent.
//! Same arrangement the extension has had since it was written, and it has caught drift.
//!
//! ## Every signal is a count, and the ones that are not are marked
//!
//! `scema-sim` scores a real expected gain only from a signal whose `measured` flag is true,
//! so that flag is a claim that somebody counted something. What is counted here:
//!
//! | Signal | Counted from |
//! |---|---|
//! | `blocked:<node>` | veto edges active from a **live** source |
//! | `unseen-units` | nodes whose source does not exist on disk |
//! | `stale-units` | nodes read past their own measured freshness budget |
//! | `gate:thin-coverage` | `Cognition::measured_fraction` |
//! | `gate:psi` | Ψ against τ_Ψ, only when the gate is measured at all |
//!
//! Nothing here estimates a severity or a probability. A "system health score" invented in
//! this module would be a hallucination with a decimal point on it, laundered into a
//! decision record that a third party can verify but cannot second-guess.
//!
//! ## An absent node is a blind spot, not a zero
//!
//! The rule this whole crate is built on, carried across the boundary intact.
//! `Provenance::Absent` becomes an entry in `blind_spots`, which `scema-sim` turns into
//! *measured* uncertainty — so an agent reasoning about a half-visible bot is less
//! confident, and can say so with a number. A node rendered as `activity: 0` would instead
//! say the unit did nothing, which is an accusation rather than an observation.

use serde_json::{json, Value};

use crate::node::{Node, Provenance, Verdict};
use crate::topology::Mesh;
use crate::{EdgeKind, TAU_PSI};

/// Name recorded in `WorldState::observer`.
///
/// Omni stamps it `imported:mesh` on the way in, so a decision record can never claim a
/// world that arrived down a pipe was observed locally.
pub const OBSERVER: &str = "mesh";

/// Signals emitted before the list is capped.
///
/// A mesh has tens of nodes, not thousands, so this is generous. It exists because a cap
/// that is never hit still has to be declared: an unbounded producer is one whose output
/// size is somebody else's problem.
const MAX_SIGNALS: usize = 64;

/// Blind spots listed before the list is truncated. Truncation is *stated*, never silent.
const MAX_BLIND_SPOTS: usize = 40;

/// Clamp into `[0, 1]`.
///
/// Omni's importer refuses a magnitude outside the unit interval, and it is right to: an
/// out-of-range magnitude would dominate a ranking through arithmetic rather than through
/// importance. Clamping here rather than being rejected there means the producer takes
/// responsibility for its own scale.
fn unit(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.0;
    }
    v.clamp(0.0, 1.0)
}

fn provenance_json(p: &Provenance) -> Value {
    match p {
        Provenance::Live { age_secs } => json!({ "kind": "live", "age_secs": age_secs }),
        Provenance::Stale { age_secs, budget_secs } => {
            json!({ "kind": "stale", "age_secs": age_secs, "budget_secs": budget_secs })
        }
        Provenance::Absent => json!({ "kind": "absent" }),
        Provenance::Simulated => json!({ "kind": "simulated" }),
    }
}

/// `scema_world::Scalar`, externally tagged as `{t, v}`.
fn text(v: impl Into<String>) -> Value {
    json!({ "t": "text", "v": v.into() })
}
fn num(v: f64) -> Value {
    json!({ "t": "num", "v": v })
}

fn signal(
    id: impl Into<String>,
    polarity: &str,
    label: impl Into<String>,
    detail: impl Into<String>,
    magnitude: f64,
    targets: Vec<String>,
    evidence: Vec<String>,
) -> Value {
    json!({
        "id": id.into(),
        "polarity": polarity,
        "label": label.into(),
        "detail": detail.into(),
        "magnitude": unit(magnitude),
        // Always true here, and every branch below supplies an evidence line saying what
        // was counted. Omni's importer refuses a `measured` signal that cites nothing,
        // which is exactly the check this producer wants to be held to.
        "measured": true,
        "targets": targets,
        "evidence": evidence,
    })
}

/// A node, as an omni object.
///
/// `activity` is carried only when the node has one. A node with `activity: None` gets no
/// attribute at all rather than a zero — `scema_world::Object` with an empty `attrs` map is
/// the representation of "we could not recover values", and that is the true statement.
fn object(n: &Node) -> Value {
    let mut attrs = serde_json::Map::new();
    attrs.insert("verdict".into(), text(format!("{:?}", n.verdict).to_lowercase()));
    attrs.insert("kind".into(), text(format!("{:?}", n.kind).to_lowercase()));
    if let Some(a) = n.activity {
        attrs.insert("activity".into(), num(unit(a)));
    }
    if let Some(r) = &n.reason {
        attrs.insert("reason".into(), text(r.clone()));
    }
    json!({
        "id": n.id,
        "kind": format!("{:?}", n.kind).to_lowercase(),
        "label": n.label,
        "attrs": Value::Object(attrs),
        "provenance": provenance_json(&n.provenance),
    })
}

/// Describe a collected mesh as a `WorldState`.
///
/// `locator` is what a decision record cites to find this environment again — the directory
/// the collector ran against. It must not be empty; omni's importer refuses a world whose
/// entity cannot be located, because a record naming an empty string is a record nobody can
/// re-check.
pub fn world(mesh: &Mesh, locator: &str, observed_at: i64) -> Value {
    let locator = if locator.trim().is_empty() { "." } else { locator.trim() };

    let objects: Vec<Value> = mesh.nodes.iter().map(object).collect();
    let mut signals: Vec<Value> = Vec::new();
    let mut blind_spots: Vec<String> = Vec::new();

    // ── what could not be seen, first ────────────────────────────────────────
    let absent: Vec<&Node> = mesh
        .nodes
        .iter()
        .filter(|n| matches!(n.provenance, Provenance::Absent))
        .collect();
    for n in absent.iter().take(MAX_BLIND_SPOTS) {
        blind_spots.push(format!(
            "{} ({}): no source on disk — unseen, not idle",
            n.id, n.label
        ));
    }
    if absent.len() > MAX_BLIND_SPOTS {
        // A silently truncated list is a wrong count, and the count is the whole point.
        blind_spots.push(format!(
            "… {} further unit(s) unseen and not listed",
            absent.len() - MAX_BLIND_SPOTS
        ));
    }

    // ── vetoes that are blocking *now* ───────────────────────────────────────
    //
    // A veto from a stale source is deliberately excluded. "The DQ* is suppressing buys"
    // and "the DQ* was suppressing buys when it last wrote, three months ago" are different
    // sentences and only the first justifies acting — the same distinction `MeshSummary`
    // draws between `blocking` and `blocking_stale`.
    for e in mesh.edges.iter().filter(|e| e.kind == EdgeKind::Veto && e.active == Some(true)) {
        let Some(source) = mesh.node(&e.from) else { continue };
        if !source.provenance.is_actionable() {
            continue;
        }
        signals.push(signal(
            format!("blocked:{}", e.from),
            "risk",
            format!("`{}` is blocking `{}`", source.label, e.to),
            // The node's own reason first, the edge label as a fallback. An edge that
            // carries neither says only that it is blocking, which is still true and still
            // worth reporting — the detail is a courtesy, not the claim.
            source
                .reason
                .clone()
                .or_else(|| e.label.clone())
                .unwrap_or_else(|| "blocking; no reason recorded on the node or the edge".to_string()),
            // A live veto is the strongest thing this observer can report and it is a
            // single, definite, counted fact, so it takes the top of the scale.
            1.0,
            vec![e.from.clone(), e.to.clone()],
            vec![format!(
                "edge {} -> {} read as active from a live source ({})",
                e.from, e.to, source.id
            )],
        ));
    }

    // ── units that are dark, and units that are old ──────────────────────────
    if !absent.is_empty() {
        let total = mesh.nodes.len().max(1);
        signals.push(signal(
            "unseen-units",
            "risk",
            format!("{} of {} unit(s) cannot be seen at all", absent.len(), mesh.nodes.len()),
            "No source file on disk. These units are unobserved, which is not the same as idle.",
            absent.len() as f64 / total as f64,
            absent.iter().map(|n| n.id.clone()).collect(),
            vec![format!(
                "counted {} node(s) with absent provenance of {} collected",
                absent.len(),
                mesh.nodes.len()
            )],
        ));
    }

    let stale: Vec<&Node> = mesh
        .nodes
        .iter()
        .filter(|n| matches!(n.provenance, Provenance::Stale { .. }))
        .collect();
    if !stale.is_empty() {
        let total = mesh.nodes.len().max(1);
        signals.push(signal(
            "stale-units",
            "risk",
            format!("{} unit(s) were last written past their own budget", stale.len()),
            "Their values were true once. They are not true now, and must not be acted on as current.",
            stale.len() as f64 / total as f64,
            stale.iter().map(|n| n.id.clone()).collect(),
            vec![format!(
                "counted {} node(s) whose age exceeds that source's measured freshness budget",
                stale.len()
            )],
        ));
    }

    // ── the gate ─────────────────────────────────────────────────────────────
    //
    // Ψ is only reported when something measured it. A gate computed entirely on neutral
    // elements says nothing about the system, and emitting it as a counted signal would
    // launder "nobody checked" into "checked and fine".
    let c = &mesh.cognition;
    if c.measured_fraction > 0.0 {
        if c.psi < TAU_PSI {
            signals.push(signal(
                "gate:psi",
                "risk",
                format!("the agentic gate reads Ψ {:.2}, below τ_Ψ {TAU_PSI:.2}", c.psi),
                c.reading.clone(),
                unit((TAU_PSI - c.psi) / TAU_PSI.max(1e-9)),
                vec![],
                vec![format!(
                    "Ψ {:.4} computed over {:.0}% measured terms",
                    c.psi,
                    c.measured_fraction * 100.0
                )],
            ));
        }
        if c.measured_fraction < 0.5 {
            // A gate standing on under half its terms is a statement about ignorance, and it
            // has to be legible as one rather than as a reassuring number.
            signals.push(signal(
                "gate:thin-coverage",
                "risk",
                format!(
                    "the gate stands on {:.0}% measured terms",
                    c.measured_fraction * 100.0
                ),
                "Ψ over mostly-neutral elements describes what has not been measured, not what is safe.",
                1.0 - c.measured_fraction,
                vec![],
                vec![format!("measured_fraction {:.4}", c.measured_fraction)],
            ));
        }
    } else {
        blind_spots.push(
            "the agentic gate: no term was measured, so Ψ describes nothing".to_string(),
        );
    }

    // ── units whose decision could not be read ───────────────────────────────
    //
    // Visible, but the verdict is undeterminable. Distinct from absent — the file is there
    // and something in it could not be understood, which is a different thing to go and fix.
    let unknown: Vec<&Node> = mesh
        .nodes
        .iter()
        .filter(|n| n.verdict == Verdict::Unknown && n.provenance.is_visible())
        .collect();
    if !unknown.is_empty() {
        signals.push(signal(
            "undetermined-verdicts",
            "risk",
            format!("{} visible unit(s) have an unreadable decision", unknown.len()),
            "Absence of a veto is not evidence of a pass.",
            unknown.len() as f64 / mesh.nodes.len().max(1) as f64,
            unknown.iter().map(|n| n.id.clone()).collect(),
            vec![format!(
                "counted {} node(s) with a visible source and Verdict::Unknown",
                unknown.len()
            )],
        ));
    }

    if signals.len() > MAX_SIGNALS {
        signals.truncate(MAX_SIGNALS);
        blind_spots.push(format!("signal list capped at {MAX_SIGNALS}; some were not emitted"));
    }

    json!({
        // The contract version. Declared rather than assumed: this crate cannot link
        // `scema-world` (that is the whole reason the world is hand-built JSON), so this
        // string is the only thing that tells an importer which reading of the format the
        // producer was written against.
        "schema": "scema.world/1",
        "observer": OBSERVER,
        "entity": {
            // `Service` rather than `Process`: what is being described is the running
            // system as a whole, not one PID. The sniper, the dashboard and the NN agent
            // are separate processes writing into one state directory.
            "kind": "service",
            "locator": locator,
            "label": "scematica",
        },
        "domain": "trading",
        "observed_at": observed_at,
        "objects": objects,
        "facts": [],
        "signals": signals,
        "extent": {
            "observed": mesh.nodes.len(),
            // The collector enumerates a fixed roster, so it knows the denominator exactly.
            // This is one of the few observers in the project that can honestly claim a
            // complete extent, and it should — an unnecessary `null` here would manufacture
            // uncertainty the same way a missing one manufactures confidence.
            "total": mesh.nodes.len(),
            "note": format!(
                "{} unit(s) in the roster; {:.0}% currently visible",
                mesh.nodes.len(),
                mesh.summary.visibility * 100.0
            ),
        },
        "blind_spots": blind_spots,
    })
}

/// Pretty-printed, for `mesh-dashboard --world`.
pub fn world_string(mesh: &Mesh, locator: &str, observed_at: i64) -> String {
    serde_json::to_string_pretty(&world(mesh, locator, observed_at))
        .unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"))
}

/// As [`world_string`], stamped with the current time.
///
/// The clock lives here rather than at the call site because this crate already depends on
/// `chrono` and `mesh-dashboard` does not. A binary that added a date library to fill in one
/// integer would be paying for the boundary rather than for the feature.
pub fn world_string_now(mesh: &Mesh, locator: &str) -> String {
    world_string(mesh, locator, chrono::Utc::now().timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::Edge;
    use crate::node::NodeKind;

    fn node(id: &str, p: Provenance, v: Verdict) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::Filter,
            label: id.into(),
            blurb: "b".into(),
            provenance: p,
            verdict: v,
            activity: None,
            detail: vec![],
            reason: None,
        }
    }

    fn mesh_of(nodes: Vec<Node>, edges: Vec<Edge>) -> Mesh {
        Mesh::new(nodes, edges, "2026-08-20T00:00:00Z".into())
    }

    /// The checks omni's `ImportObserver` runs, restated here so this crate fails its own
    /// build rather than producing something the consumer rejects at run time.
    ///
    /// Kept as an explicit list rather than a dependency on `scema-tools`: the whole reason
    /// this module emits hand-built JSON is that the two workspaces do not link, and a test
    /// that reached across would quietly reintroduce the coupling it exists to avoid.
    fn assert_importable(w: &Value) {
        // The contract version. Restated here rather than imported, for the same reason as
        // everything else in this function: this crate cannot link `scema-world`. A
        // producer that stops declaring it is refused by the importer, and the point of a
        // self-check is to fail here first.
        assert_eq!(w["schema"].as_str(), Some("scema.world/1"));
        assert!(w["observer"].as_str().is_some_and(|s| !s.trim().is_empty()));
        assert!(w["entity"]["locator"].as_str().is_some_and(|s| !s.trim().is_empty()));
        assert!(w["observed_at"].is_i64());

        let signals = w["signals"].as_array().expect("signals must be an array");
        let mut ids: Vec<&str> = Vec::new();
        for s in signals {
            let id = s["id"].as_str().expect("signal id");
            assert!(!id.trim().is_empty());
            ids.push(id);

            let m = s["magnitude"].as_f64().expect("magnitude");
            assert!((0.0..=1.0).contains(&m), "magnitude {m} outside [0,1] on `{id}`");

            assert!(matches!(s["polarity"].as_str(), Some("risk") | Some("opportunity")));

            // The one that matters: a counted signal must cite its count, or it is a guess
            // wearing a measurement's clothes.
            if s["measured"] == json!(true) {
                let ev = s["evidence"].as_array().expect("evidence");
                assert!(!ev.is_empty(), "`{id}` claims measured and cites nothing");
            }
        }
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "duplicate signal id: --ground could not name it");

        let mut object_ids: Vec<&str> =
            w["objects"].as_array().unwrap().iter().map(|o| o["id"].as_str().unwrap()).collect();
        object_ids.sort_unstable();
        let before = object_ids.len();
        object_ids.dedup();
        assert_eq!(before, object_ids.len(), "duplicate object id");

        let observed = w["extent"]["observed"].as_u64().unwrap();
        if let Some(total) = w["extent"]["total"].as_u64() {
            assert!(observed <= total, "extent numerator exceeds its denominator");
        }
    }

    #[test]
    fn a_mesh_world_is_accepted_by_the_shape_the_importer_enforces() {
        let m = mesh_of(
            vec![
                node("a", Provenance::Live { age_secs: 1 }, Verdict::Pass),
                node("b", Provenance::Stale { age_secs: 900, budget_secs: 60 }, Verdict::Pass),
                node("c", Provenance::Absent, Verdict::Unknown),
            ],
            vec![],
        );
        assert_importable(&world(&m, "/bot", 1_700_000_000));
    }

    #[test]
    fn an_absent_node_becomes_a_blind_spot_and_never_an_activity_of_zero() {
        // The rule the whole crate is built on, carried across the boundary intact. An
        // object with `activity: 0` says the unit did nothing; a blind spot says we could
        // not see it. One is an accusation.
        let m = mesh_of(vec![node("dark", Provenance::Absent, Verdict::Unknown)], vec![]);
        let w = world(&m, "/bot", 0);
        let spots = w["blind_spots"].as_array().unwrap();
        assert!(spots.iter().any(|b| b.as_str().unwrap().contains("dark")), "{w:#}");
        let obj = &w["objects"][0];
        assert!(obj["attrs"].get("activity").is_none(), "an unseen unit must carry no activity");
        assert_eq!(obj["provenance"]["kind"], json!("absent"));
    }

    #[test]
    fn a_live_veto_is_a_counted_signal_and_a_stale_one_is_not() {
        // `MeshSummary` already draws this distinction between `blocking` and
        // `blocking_stale`; a signal that erased it would have omni acting on a veto that
        // was true three months ago.
        let live = mesh_of(
            vec![
                node("v", Provenance::Live { age_secs: 2 }, Verdict::Veto),
                node("t", Provenance::Live { age_secs: 2 }, Verdict::Pass),
            ],
            vec![Edge::veto("v", "t").with_active(Some(true)).with_label("suppressing")],
        );
        let w = world(&live, "/bot", 0);
        assert!(
            w["signals"].as_array().unwrap().iter().any(|s| s["id"] == json!("blocked:v")),
            "{w:#}"
        );

        let stale = mesh_of(
            vec![
                node("v", Provenance::Stale { age_secs: 9_000, budget_secs: 60 }, Verdict::Veto),
                node("t", Provenance::Live { age_secs: 2 }, Verdict::Pass),
            ],
            vec![Edge::veto("v", "t").with_active(Some(true)).with_label("suppressing")],
        );
        let w = world(&stale, "/bot", 0);
        assert!(
            !w["signals"].as_array().unwrap().iter().any(|s| s["id"] == json!("blocked:v")),
            "a veto from a stale source must not read as blocking now: {w:#}"
        );
    }

    #[test]
    fn an_unmeasured_gate_is_a_blind_spot_rather_than_a_reassuring_psi() {
        // `Mesh::new` evaluates the gate with no signals, so nothing is measured. Emitting
        // a counted `gate:psi` from that would launder "nobody checked" into "checked".
        let m = mesh_of(vec![node("a", Provenance::Live { age_secs: 1 }, Verdict::Pass)], vec![]);
        let w = world(&m, "/bot", 0);
        assert!(
            w["blind_spots"]
                .as_array()
                .unwrap()
                .iter()
                .any(|b| b.as_str().unwrap().contains("agentic gate")),
            "{w:#}"
        );
        assert!(!w["signals"].as_array().unwrap().iter().any(|s| s["id"] == json!("gate:psi")));
    }

    #[test]
    fn a_visible_unit_with_an_unreadable_verdict_is_counted_separately_from_an_absent_one() {
        // Absence of a veto is not evidence of a pass, and "the file is missing" and "the
        // file is there and I could not understand it" send an operator to two places.
        let m = mesh_of(
            vec![
                node("seen", Provenance::Live { age_secs: 1 }, Verdict::Unknown),
                node("dark", Provenance::Absent, Verdict::Unknown),
            ],
            vec![],
        );
        let w = world(&m, "/bot", 0);
        let s = w["signals"]
            .as_array()
            .unwrap()
            .iter()
            .find(|s| s["id"] == json!("undetermined-verdicts"))
            .expect("the visible-but-unreadable case must be counted");
        assert_eq!(s["targets"], json!(["seen"]), "the absent node belongs in blind_spots");
    }

    #[test]
    fn the_extent_denominator_is_known_because_the_roster_is_fixed() {
        // One of the few observers in the project that can honestly claim completeness. An
        // unnecessary `null` here manufactures uncertainty exactly as a missing one
        // manufactures confidence.
        let m = mesh_of(
            vec![
                node("a", Provenance::Live { age_secs: 1 }, Verdict::Pass),
                node("b", Provenance::Absent, Verdict::Unknown),
            ],
            vec![],
        );
        let w = world(&m, "/bot", 0);
        assert_eq!(w["extent"]["observed"], json!(2));
        assert_eq!(w["extent"]["total"], json!(2));
    }

    #[test]
    fn an_empty_locator_becomes_a_usable_one_rather_than_an_unciteable_record() {
        let m = mesh_of(vec![], vec![]);
        assert_eq!(world(&m, "   ", 0)["entity"]["locator"], json!("."));
    }

    #[test]
    fn every_signal_carries_a_count_in_its_evidence() {
        // The property `scema-sim` depends on to score a real expected gain. Asserted over a
        // mesh built to trip every branch at once.
        let m = mesh_of(
            vec![
                node("v", Provenance::Live { age_secs: 1 }, Verdict::Veto),
                node("t", Provenance::Live { age_secs: 1 }, Verdict::Pass),
                node("s", Provenance::Stale { age_secs: 900, budget_secs: 60 }, Verdict::Pass),
                node("d", Provenance::Absent, Verdict::Unknown),
                node("u", Provenance::Live { age_secs: 1 }, Verdict::Unknown),
            ],
            vec![Edge::veto("v", "t").with_active(Some(true)).with_label("suppressing")],
        );
        let w = world(&m, "/bot", 0);
        let signals = w["signals"].as_array().unwrap();
        assert!(signals.len() >= 4, "{w:#}");
        for s in signals {
            assert_eq!(s["measured"], json!(true));
            let first = s["evidence"][0].as_str().unwrap_or_default();
            assert!(
                first.contains("counted") || first.contains("read as") || first.contains("Ψ")
                    || first.contains("measured_fraction"),
                "`{}` cites nothing countable: {first}",
                s["id"]
            );
        }
        assert_importable(&w);
    }
}
