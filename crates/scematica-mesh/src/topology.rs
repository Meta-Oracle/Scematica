//! The mesh itself: nodes, edges, and the two summary figures that belong above them.

use serde::{Deserialize, Serialize};

use crate::cognition::{self, Cognition, Signals};
use crate::edge::Edge;
use crate::node::{Node, Provenance, Verdict};

/// Counts and the headline, computed rather than stored.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MeshSummary {
    pub nodes_total: usize,
    pub nodes_live: usize,
    pub nodes_stale: usize,
    pub nodes_absent: usize,
    pub nodes_simulated: usize,
    /// Fraction of nodes with usable current data, 0..1.
    ///
    /// This is the number that goes at the top of the page, above any performance figure,
    /// for the same reason the Ψ gate exists: a confident reading of a system you can only
    /// half see is worth less than knowing you can only half see it. It is deliberately
    /// **not** a health score — a system can be perfectly healthy and mostly invisible,
    /// and a reader must be able to tell those apart.
    pub visibility: f64,
    /// Veto edges blocking right now, from a source that is currently readable.
    ///
    /// A veto whose source node is **stale** is deliberately not counted here. Its data
    /// was true once and may still be, but "the DQ* is suppressing buys" and "the DQ* was
    /// suppressing buys when it last wrote, three months ago" are different sentences and
    /// only the first justifies acting. Those land in [`Self::blocking_stale`].
    pub blocking: usize,
    /// Veto edges that were active as of a stale reading.
    pub blocking_stale: usize,
    /// One sentence naming what is stopping the system, when something is.
    pub diagnosis: String,
}

/// A complete observation of the running system.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mesh {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// RFC3339 timestamp of the collection pass.
    pub generated_at: String,
    pub summary: MeshSummary,
    /// The agentic gate (§32) evaluated over these nodes. See [`crate::cognition`].
    pub cognition: Cognition,
}

impl Mesh {
    /// Assemble a mesh and compute its summary, with no numeric signals.
    ///
    /// The gate will be almost entirely unmeasured, which is the correct reading for a
    /// topology assembled without them — and it will say so via
    /// [`Cognition::measured_fraction`] rather than quietly reporting a confident Ψ.
    pub fn new(nodes: Vec<Node>, edges: Vec<Edge>, generated_at: String) -> Self {
        Self::with_signals(nodes, edges, generated_at, &Signals::default())
    }

    /// Assemble a mesh, evaluating the gate against the numeric signals the collector
    /// recovered from the state files.
    pub fn with_signals(
        nodes: Vec<Node>,
        edges: Vec<Edge>,
        generated_at: String,
        signals: &Signals,
    ) -> Self {
        let summary = summarise(&nodes, &edges);
        let cognition = cognition::assess(&nodes, signals);
        Mesh { nodes, edges, generated_at, summary, cognition }
    }

    pub fn node(&self, id: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Structural problems with this graph.
    ///
    /// An edge naming a node that does not exist is the failure mode of a hand-written
    /// topology: it renders as a line into empty space, or vanishes silently depending on
    /// the layout engine, and either way the picture stops matching the system. Checked
    /// here so a typo fails a test rather than quietly deleting an arrow.
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        for e in &self.edges {
            if self.node(&e.from).is_none() {
                problems.push(format!("edge {} -> {} has no source node", e.from, e.to));
            }
            if self.node(&e.to).is_none() {
                problems.push(format!("edge {} -> {} has no target node", e.from, e.to));
            }
        }
        let mut seen: Vec<&str> = Vec::new();
        for n in &self.nodes {
            if seen.contains(&n.id.as_str()) {
                problems.push(format!("duplicate node id {}", n.id));
            }
            seen.push(&n.id);
        }
        problems
    }
}

fn summarise(nodes: &[Node], edges: &[Edge]) -> MeshSummary {
    let total = nodes.len();
    let mut live = 0;
    let mut stale = 0;
    let mut absent = 0;
    let mut simulated = 0;
    for n in nodes {
        match n.provenance {
            Provenance::Live { .. } => live += 1,
            Provenance::Stale { .. } => stale += 1,
            Provenance::Absent => absent += 1,
            Provenance::Simulated => simulated += 1,
        }
    }
    let visibility = if total == 0 { 0.0 } else { live as f64 / total as f64 };

    // A block only counts as current when the unit asserting it can still be read.
    let source_live = |e: &Edge| {
        nodes
            .iter()
            .find(|n| n.id == e.from)
            .map(|n| n.provenance.is_actionable())
            .unwrap_or(false)
    };
    let blocking = edges.iter().filter(|e| e.is_blocking() && source_live(e)).count();
    let blocking_stale = edges.iter().filter(|e| e.is_blocking() && !source_live(e)).count();

    MeshSummary {
        nodes_total: total,
        nodes_live: live,
        nodes_stale: stale,
        nodes_absent: absent,
        nodes_simulated: simulated,
        visibility,
        blocking,
        blocking_stale,
        diagnosis: diagnose(nodes, edges, visibility),
    }
}

/// The one sentence worth reading first.
///
/// Ordered by what actually stops a trade, most specific first. An active veto beats a
/// visibility complaint because the operator can act on it; "you can only see 40% of the
/// system" is true but useless when the answer is right there in a node's `reason`.
fn diagnose(nodes: &[Node], edges: &[Edge], visibility: f64) -> String {
    // A named veto, with the vetoing node's own explanation. Tense follows the source's
    // provenance: a live source speaks in the present, a stale one in the past with its
    // age attached, because the reader's next action differs completely between the two.
    if let Some(edge) = edges.iter().find(|e| e.is_blocking()) {
        let who = nodes.iter().find(|n| n.id == edge.from);
        let label = who.map(|n| n.label.as_str()).unwrap_or(edge.from.as_str());
        let why = who
            .and_then(|n| n.reason.clone())
            .unwrap_or_else(|| "blocking".to_string());
        return match who.map(|n| &n.provenance) {
            Some(Provenance::Live { .. }) => format!("{label} is blocking: {why}"),
            Some(Provenance::Stale { age_secs, .. }) => format!(
                "{label} was blocking when it last wrote {} ago: {why} — this reading is stale, \
                 so treat it as the last known state rather than the current one",
                humanise(*age_secs)
            ),
            _ => format!("{label} was blocking, but its source can no longer be read: {why}"),
        };
    }

    // Nothing blocking, but degraded inputs somewhere.
    if let Some(n) = nodes.iter().find(|n| n.verdict == Verdict::Degraded) {
        return format!(
            "{} is running on degraded inputs: {}",
            n.label,
            n.reason.clone().unwrap_or_else(|| "unspecified".into())
        );
    }

    // Nothing current to report. Distinguish the three ways that happens, because they
    // call for completely different actions: nothing was ever written, everything written
    // has gone cold, or the system is genuinely healthy and quiet.
    let any_visible = nodes.iter().any(|n| n.provenance.is_visible());
    if !any_visible {
        return "nothing is visible — no state files found, so the system is either not \
                running or is running somewhere this collector cannot see"
            .to_string();
    }

    if visibility == 0.0 {
        let oldest = nodes
            .iter()
            .filter_map(|n| match n.provenance {
                Provenance::Stale { age_secs, .. } => Some(age_secs),
                _ => None,
            })
            .max();
        return match oldest {
            Some(age) => format!(
                "every readable unit is stale — the newest state file is {} old, so this is a \
                 snapshot of a system that has stopped, not a picture of one running",
                humanise(age)
            ),
            None => "no unit is currently readable".to_string(),
        };
    }

    if visibility < 0.5 {
        return format!(
            "no active veto, but only {:.0}% of the mesh is readable — treat the rest as unknown, not idle",
            visibility * 100.0
        );
    }

    "no unit is blocking flow".to_string()
}

/// Coarse human duration. Deliberately coarse: a veto that has stood for "3h" and one that
/// has stood for "3h 14m" call for the same action, and the extra precision only invites
/// the reader to treat a staleness figure as a measurement.
fn humanise(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3_600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3_600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::Edge;
    use crate::node::NodeKind;

    fn node(id: &str, p: Provenance) -> Node {
        Node {
            id: id.into(),
            kind: NodeKind::Filter,
            label: id.into(),
            blurb: String::new(),
            provenance: p,
            verdict: Verdict::Pass,
            activity: None,
            detail: vec![],
            reason: None,
        }
    }

    #[test]
    fn visibility_counts_only_live_nodes() {
        let m = Mesh::new(
            vec![
                node("a", Provenance::Live { age_secs: 1 }),
                node("b", Provenance::Stale { age_secs: 99, budget_secs: 5 }),
                node("c", Provenance::Absent),
                node("d", Provenance::Live { age_secs: 2 }),
            ],
            vec![],
            "t".into(),
        );
        assert_eq!(m.summary.nodes_live, 2);
        assert_eq!(m.summary.nodes_stale, 1);
        assert_eq!(m.summary.nodes_absent, 1);
        assert_eq!(m.summary.visibility, 0.5);
    }

    /// The diagnosis must name the blocker and quote its own reason, because a page that
    /// says "something is wrong" sends the reader back to grepping JSON — which is the
    /// workflow this feature replaces.
    #[test]
    fn an_active_veto_names_itself_in_the_diagnosis() {
        let mut n = node("learner.dqstar", Provenance::Live { age_secs: 1 });
        n.label = "DQ*".into();
        n.verdict = Verdict::Veto;
        n.reason = Some("bearish Q leads the best buy by 3.4x".into());
        let m = Mesh::new(
            vec![n, node("exec", Provenance::Live { age_secs: 1 })],
            vec![Edge::veto("learner.dqstar", "exec").with_active(Some(true))],
            "t".into(),
        );
        assert_eq!(m.summary.blocking, 1);
        assert!(m.summary.diagnosis.contains("DQ*"));
        assert!(m.summary.diagnosis.contains("3.4x"));
    }

    /// The condition this page is most often read in: the bot is not running, so every
    /// file on disk is old. A veto recovered from a stale file is a fact about the past
    /// and must be stated in the past tense with its age — otherwise the operator chases
    /// a gate that may have been open for months.
    #[test]
    fn a_stale_veto_is_reported_in_the_past_tense_and_not_counted_as_current() {
        let mut n = node("learner.dqstar", Provenance::Stale { age_secs: 7_000_000, budget_secs: 120 });
        n.label = "DQ*".into();
        n.verdict = Verdict::Veto;
        n.reason = Some("bearish Q leads the best buy by 3.4x".into());
        let m = Mesh::new(
            vec![n, node("exec", Provenance::Stale { age_secs: 7_000_000, budget_secs: 30 })],
            vec![Edge::veto("learner.dqstar", "exec").with_active(Some(true))],
            "t".into(),
        );
        assert_eq!(m.summary.blocking, 0, "a stale block is not a current block");
        assert_eq!(m.summary.blocking_stale, 1);
        assert!(m.summary.diagnosis.contains("was blocking"), "got: {}", m.summary.diagnosis);
        assert!(m.summary.diagnosis.contains("81d"), "the age must be on screen: {}", m.summary.diagnosis);
        assert!(m.summary.diagnosis.contains("stale"));
    }

    /// All files present but cold is a different statement from no files at all, and the
    /// remedy differs: one means start the bot, the other means look somewhere else.
    #[test]
    fn everything_stale_is_distinguished_from_nothing_present() {
        let cold = Mesh::new(
            vec![node("a", Provenance::Stale { age_secs: 90_000, budget_secs: 30 })],
            vec![],
            "t".into(),
        );
        assert!(cold.summary.diagnosis.contains("has stopped"), "got: {}", cold.summary.diagnosis);

        let empty = Mesh::new(vec![node("a", Provenance::Absent)], vec![], "t".into());
        assert!(empty.summary.diagnosis.contains("no state files found"));
    }

    /// A veto whose state could not be read must not produce a confident "all clear".
    #[test]
    fn an_unknown_veto_does_not_report_all_clear_as_a_block() {
        let m = Mesh::new(
            vec![node("a", Provenance::Live { age_secs: 1 }), node("b", Provenance::Live { age_secs: 1 })],
            vec![Edge::veto("a", "b")],
            "t".into(),
        );
        assert_eq!(m.summary.blocking, 0);
        assert_eq!(m.summary.diagnosis, "no unit is blocking flow");
    }

    #[test]
    fn an_empty_system_says_so_rather_than_reporting_health() {
        let m = Mesh::new(vec![node("a", Provenance::Absent)], vec![], "t".into());
        assert_eq!(m.summary.visibility, 0.0);
        assert!(m.summary.diagnosis.contains("nothing is visible"));
    }

    #[test]
    fn low_visibility_is_reported_when_nothing_blocks() {
        let m = Mesh::new(
            vec![
                node("a", Provenance::Live { age_secs: 1 }),
                node("b", Provenance::Absent),
                node("c", Provenance::Absent),
            ],
            vec![],
            "t".into(),
        );
        assert!(m.summary.diagnosis.contains("only 33%"), "got: {}", m.summary.diagnosis);
    }

    #[test]
    fn dangling_edges_are_a_validation_error() {
        let m = Mesh::new(vec![node("a", Provenance::Absent)], vec![Edge::signal("a", "ghost")], "t".into());
        let problems = m.validate();
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("ghost"));
    }

    #[test]
    fn duplicate_node_ids_are_a_validation_error() {
        let m = Mesh::new(
            vec![node("a", Provenance::Absent), node("a", Provenance::Absent)],
            vec![],
            "t".into(),
        );
        assert!(m.validate().iter().any(|p| p.contains("duplicate")));
    }
}
