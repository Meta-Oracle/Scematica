//! What the agent actually did, as opposed to what it decided.
//!
//! A [`scema_verify::DecisionRecord`] proves what was *chosen*. The moment the runtime acts
//! there is a second thing to prove — what was *done*, and whether it matched — and these
//! must be two records with two commitments. One record covering both would mean a failed
//! action silently rewrites the history of the decision that ordered it, which is the exact
//! failure the decision record exists to prevent.
//!
//! ## The arm that matters
//!
//! [`Outcome::Unknown`] is the reason this crate is shaped the way it is. An effect whose
//! result could not be observed is not a success and it is not a failure: the process was
//! killed between the write and the confirmation, the command exited but its output could
//! not be read, the file was renamed by something else in between. Every other layer of this
//! runtime can say "I don't know" and it costs nothing; the action path is the layer where
//! saying it is hardest and matters most, because the tempting default — assume it worked —
//! is the one that produces a record asserting something nobody checked.
//!
//! ## What is committed, and what deliberately is not
//!
//! The commitment covers the **intent** (which decision this claims to carry out), the
//! **effect** (what was to be done), and the **outcome** (what happened). It does not cover
//! `at` or `runtime`, matching `DecisionRecord` — those describe the recording, not the
//! thing recorded, and binding them would make an otherwise identical effect produce a
//! different digest on a different machine.
//!
//! ## Two gates, still separate
//!
//! Nothing here decides whether an effect is permitted. [`scema_trust`] answers *whether*
//! and `scema_tools::Workspace` answers *where*; this crate records what happened once both
//! have said yes. Keeping the recorder ignorant of the policy is deliberate — a recorder
//! that could also authorise would eventually be asked to.

pub mod exec;

use scema_verify::canonical::{digest, digest_of_digests, Digest};
use serde::{Deserialize, Serialize};

/// A declared action.
///
/// Deliberately a small, closed vocabulary. An open one — "run this arbitrary thing" as the
/// only arm — would make the approval prompt unable to describe what it is asking about,
/// and a prompt that cannot describe the act is decorative. Adding an arm is a deliberate
/// act with a risk classification attached, which is the same reason `scema_trust::Risk` is
/// declared per tool rather than inferred.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    /// Write `contents` to `path`, creating or replacing it.
    WriteFile { path: String, contents: String },
    /// Create a directory, and any missing parents.
    CreateDir { path: String },
    /// Run a command as an argv. Never a shell string — see [`Effect::argv`].
    Run { argv: Vec<String>, cwd: String },
}

impl Effect {
    /// The risk class this effect carries, for [`scema_trust`].
    ///
    /// A method rather than a field so a new arm cannot be added without the compiler
    /// demanding a classification for it.
    pub fn risk(&self) -> scema_trust::Risk {
        match self {
            Effect::WriteFile { .. } | Effect::CreateDir { .. } => scema_trust::Risk::Write,
            Effect::Run { .. } => scema_trust::Risk::Execute,
        }
    }

    /// The path this effect touches, for workspace confinement and the grant key.
    pub fn path(&self) -> &str {
        match self {
            Effect::WriteFile { path, .. } | Effect::CreateDir { path } => path,
            Effect::Run { cwd, .. } => cwd,
        }
    }

    /// The argv, when this is a command.
    ///
    /// An argv and never a command line. No pipes, no `;`, no second parsing layer between
    /// the string an approval prompt displayed and the thing that runs — if those two can
    /// differ, the prompt was decoration.
    pub fn argv(&self) -> Option<&[String]> {
        match self {
            Effect::Run { argv, .. } => Some(argv),
            _ => None,
        }
    }

    /// One line, for a prompt and for the record.
    pub fn summary(&self) -> String {
        match self {
            Effect::WriteFile { path, contents } => {
                format!("write {} ({} bytes)", path, contents.len())
            }
            Effect::CreateDir { path } => format!("create directory {path}"),
            Effect::Run { argv, cwd } => format!("run `{}` in {}", argv.join(" "), cwd),
        }
    }
}

/// What happened when an effect was carried out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Outcome {
    /// It was carried out and the result was observed.
    Succeeded { detail: String },
    /// It was attempted and failed, with the reason.
    Failed { reason: String },
    /// **It was attempted and the result could not be observed.**
    ///
    /// Not a success and not a failure. The honest arm, and the one a caller is tempted to
    /// collapse into one of the others because it is inconvenient. A record that claims
    /// success for an unobserved write is worse than no record: it is a false statement
    /// carrying a valid commitment.
    Unknown { why: String },
    /// It was not attempted, because something refused it.
    ///
    /// Carries which gate refused, since "policy refused" and "the operator declined" are
    /// different claims and only one of them is about a person.
    Refused { by: RefusedBy, reason: String },
    /// It was not attempted because this was a dry run.
    Simulated,
}

/// Which gate stopped an effect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefusedBy {
    /// `scema_tools::Workspace` — outside the roots, or a protected path.
    Workspace,
    /// `scema_trust::TrustPolicy` — a rule or a hard refusal fired. No prompt was shown.
    Policy,
    /// A prompt was shown and the answer was no.
    Operator,
}

impl Outcome {
    /// Did the world change?
    ///
    /// `Unknown` answers `false` here and that is not the same as "nothing happened" — it
    /// is "this cannot be relied upon to have happened". Callers deciding whether to
    /// continue a sequence must treat it as a stop, not as a skip.
    pub fn changed_the_world(&self) -> bool {
        matches!(self, Outcome::Succeeded { .. })
    }

    /// Is this a settled answer, either way?
    ///
    /// `false` only for [`Outcome::Unknown`]. The one question worth asking separately.
    pub fn settled(&self) -> bool {
        !matches!(self, Outcome::Unknown { .. })
    }

    pub fn label(&self) -> &'static str {
        match self {
            Outcome::Succeeded { .. } => "SUCCEEDED",
            Outcome::Failed { .. } => "FAILED",
            Outcome::Unknown { .. } => "UNKNOWN",
            Outcome::Refused { .. } => "REFUSED",
            Outcome::Simulated => "SIMULATED",
        }
    }
}

/// Digests over each committed part, plus the root binding them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectCommitment {
    /// The decision record root this effect claims to carry out.
    pub intent: String,
    pub effect: String,
    pub outcome: String,
    /// Digest over the three above, with their field names. Anchor this.
    pub root: String,
}

/// One effect, attempted, preserved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffectRecord {
    /// Short id from [`EffectCommitment::root`].
    pub id: String,
    /// Unix seconds. Outside the commitment: it describes the recording, not the act.
    pub at: i64,
    pub runtime: String,
    /// The decision this claims to carry out.
    ///
    /// A string rather than an embedded record: an effect record that carried a copy of the
    /// decision could disagree with the stored one, and then there are two histories. This
    /// is a reference, and `scema verify` checks the decision separately.
    pub intent: String,
    pub effect: Effect,
    pub outcome: Outcome,
    pub commitment: EffectCommitment,
}

fn commit(intent: &str, effect: &Effect, outcome: &Outcome) -> (EffectCommitment, Digest) {
    let i = digest(&intent);
    let e = digest(effect);
    let o = digest(outcome);
    let root = digest_of_digests(&[("intent", i), ("effect", e), ("outcome", o)]);
    (
        EffectCommitment {
            intent: i.to_hex(),
            effect: e.to_hex(),
            outcome: o.to_hex(),
            root: root.to_hex(),
        },
        root,
    )
}

impl EffectRecord {
    /// Seal an attempt.
    ///
    /// Committed separately from the decision even though it names one, so a verifier can
    /// say *which* moved. A single digest over both would say only "something changed" —
    /// the same reasoning that keeps `policy` and `decision` apart in a decision record.
    pub fn seal(
        runtime: impl Into<String>,
        at: i64,
        intent: impl Into<String>,
        effect: Effect,
        outcome: Outcome,
    ) -> Self {
        let intent = intent.into();
        let (commitment, root) = commit(&intent, &effect, &outcome);
        EffectRecord {
            id: root.short(),
            at,
            runtime: runtime.into(),
            intent,
            effect,
            outcome,
            commitment,
        }
    }
}

/// A field whose stored digest does not match its stored payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mismatch {
    pub field: String,
    pub committed: String,
    pub recomputed: String,
}

/// The result of re-deriving an effect record's commitment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verification {
    pub valid: bool,
    pub mismatches: Vec<Mismatch>,
    /// True when the parts verify but the root does not — a hand-edited root.
    pub root_only: bool,
}

/// Recompute the commitment and report what moved.
pub fn verify(record: &EffectRecord) -> Verification {
    let (fresh, root) = commit(&record.intent, &record.effect, &record.outcome);
    let mut mismatches = Vec::new();
    let mut check = |field: &str, committed: &str, recomputed: &str| {
        if committed != recomputed {
            mismatches.push(Mismatch {
                field: field.to_string(),
                committed: committed.to_string(),
                recomputed: recomputed.to_string(),
            });
        }
    };
    check("intent", &record.commitment.intent, &fresh.intent);
    check("effect", &record.commitment.effect, &fresh.effect);
    check("outcome", &record.commitment.outcome, &fresh.outcome);

    let root_matches = record.commitment.root == root.to_hex();
    Verification {
        valid: mismatches.is_empty() && root_matches,
        root_only: mismatches.is_empty() && !root_matches,
        mismatches,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect() -> Effect {
        Effect::WriteFile { path: "docs/plan.md".into(), contents: "hello".into() }
    }

    #[test]
    fn an_unobserved_result_is_neither_success_nor_failure() {
        // The arm this crate is shaped around, and the one a caller is tempted to collapse.
        let u = Outcome::Unknown { why: "killed between write and fsync".into() };
        assert!(!u.changed_the_world());
        assert!(!u.settled());
        assert!(Outcome::Succeeded { detail: String::new() }.settled());
        assert!(Outcome::Failed { reason: String::new() }.settled());
    }

    #[test]
    fn unknown_does_not_read_as_nothing_happened() {
        // `changed_the_world() == false` for both Unknown and Refused, but only one of them
        // means the world is untouched. Callers must not treat the pair as interchangeable,
        // so the distinction is kept in `settled`.
        let refused =
            Outcome::Refused { by: RefusedBy::Policy, reason: "execution is off".into() };
        assert!(!refused.changed_the_world());
        assert!(refused.settled(), "a refusal is a settled answer; an unknown is not");
    }

    #[test]
    fn every_effect_declares_a_risk() {
        // A method rather than a field, so a new arm cannot arrive unclassified.
        assert_eq!(effect().risk(), scema_trust::Risk::Write);
        assert_eq!(
            Effect::CreateDir { path: "a".into() }.risk(),
            scema_trust::Risk::Write
        );
        assert_eq!(
            Effect::Run { argv: vec!["ls".into()], cwd: ".".into() }.risk(),
            scema_trust::Risk::Execute
        );
    }

    #[test]
    fn a_command_is_an_argv_and_never_a_string() {
        // If the string a prompt displayed and the thing that runs can differ, the prompt
        // was decoration.
        let e = Effect::Run { argv: vec!["git".into(), "status".into()], cwd: ".".into() };
        assert_eq!(e.argv().unwrap(), &["git".to_string(), "status".to_string()]);
        assert_eq!(effect().argv(), None);
    }

    #[test]
    fn a_sealed_record_verifies() {
        let r = EffectRecord::seal(
            "scema-omni/test",
            0,
            "abc123",
            effect(),
            Outcome::Succeeded { detail: "5 bytes".into() },
        );
        let v = verify(&r);
        assert!(v.valid, "{v:?}");
        assert!(v.mismatches.is_empty());
        assert_eq!(r.id.len(), 8);
    }

    #[test]
    fn changing_the_outcome_is_caught_and_named() {
        // The tampering this record exists to expose: an attempt that failed, edited later
        // to say it succeeded.
        let mut r = EffectRecord::seal(
            "scema-omni/test",
            0,
            "abc123",
            effect(),
            Outcome::Failed { reason: "permission denied".into() },
        );
        r.outcome = Outcome::Succeeded { detail: "done".into() };
        let v = verify(&r);
        assert!(!v.valid);
        assert_eq!(v.mismatches.len(), 1);
        assert_eq!(v.mismatches[0].field, "outcome");
    }

    #[test]
    fn repointing_an_effect_at_a_different_decision_is_caught() {
        // An effect record is a claim about which decision authorised it. Moving that
        // pointer is exactly as serious as editing the outcome.
        let mut r = EffectRecord::seal("rt", 0, "abc123", effect(), Outcome::Simulated);
        r.intent = "def456".into();
        let v = verify(&r);
        assert!(!v.valid);
        assert_eq!(v.mismatches[0].field, "intent");
    }

    #[test]
    fn a_hand_edited_root_is_flagged_separately() {
        // The parts all verify and only the root is wrong, which is a different diagnosis
        // from a field having moved — and it is what somebody does when they have edited a
        // field and then recomputed nothing.
        let mut r = EffectRecord::seal("rt", 0, "abc123", effect(), Outcome::Simulated);
        r.commitment.root = "0".repeat(64);
        let v = verify(&r);
        assert!(!v.valid);
        assert!(v.root_only);
        assert!(v.mismatches.is_empty());
    }

    #[test]
    fn the_recording_metadata_is_outside_the_commitment() {
        // `at` and `runtime` describe the recording, not the act. Binding them would make an
        // otherwise identical effect hash differently on another machine.
        let a = EffectRecord::seal("rt-a", 0, "abc", effect(), Outcome::Simulated);
        let b = EffectRecord::seal("rt-b", 999, "abc", effect(), Outcome::Simulated);
        assert_eq!(a.commitment.root, b.commitment.root);
        assert_eq!(a.id, b.id);
    }

    #[test]
    fn a_dry_run_is_its_own_outcome_and_never_a_success() {
        // Simulated has to be distinguishable in the record, or a dry run's output becomes
        // indistinguishable from a real one after the fact — which is the whole reason
        // simulate and decide are different keystrokes upstream.
        assert!(!Outcome::Simulated.changed_the_world());
        assert_eq!(Outcome::Simulated.label(), "SIMULATED");
    }

    #[test]
    fn a_refusal_records_which_gate_said_no() {
        // "Policy refused" and "the operator declined" are different claims, and only one of
        // them is about a person. Same distinction as `scema_trust::Refusal`.
        for by in [RefusedBy::Workspace, RefusedBy::Policy, RefusedBy::Operator] {
            let r = EffectRecord::seal(
                "rt",
                0,
                "abc",
                effect(),
                Outcome::Refused { by, reason: "x".into() },
            );
            assert!(verify(&r).valid);
        }
    }
}
