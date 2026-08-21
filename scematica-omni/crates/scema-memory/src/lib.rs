//! # scema-memory — four memories, one of which is unusual
//!
//! A vector store is not a memory; it is a search index over things that were said. This
//! crate keeps four kinds, separated because they answer different questions and decay at
//! different rates:
//!
//! | Kind | Question | Example |
//! |---|---|---|
//! | [`MemoryBody::Episode`] | what happened? | "deployed X, it failed, cause was an RPC timeout" |
//! | [`MemoryBody::Belief`] | what do I hold to be true? | "this RPC provider degrades under load" |
//! | [`MemoryBody::Procedure`] | how is this done? | "seed pools → verify graph → run arb" |
//! | [`MemoryBody::Counterfactual`] | what would the branch I rejected have done? | "H₂ was projected at 0.31 and not taken" |
//!
//! ## The fourth one is the point, and it is mostly unanswerable
//!
//! A counterfactual records a branch the agent *declined*. Its projected utility is known —
//! it was computed — and its realised utility almost never is, because nobody ran it. That
//! asymmetry is the design, not a gap to fill in later.
//!
//! It is the same asymmetry the bot's own `calibration.rs` lives with: a bullish call
//! resolves against realised PnL, a bearish one almost never resolves because the bot
//! avoided that pool. The rule that falls out of it is the one that matters:
//!
//! > **Unresolved counterfactuals are counted, never scored.**
//!
//! [`Calibration`] therefore reports `resolved` and `unresolved` as separate integers and
//! computes error only over the first. An implementation that imputed outcomes for
//! untaken branches — from a model, from a neighbour, from a prior — would be generating
//! its own training signal, and every subsequent decision would be tuned to a fiction.
//!
//! ## Storage
//!
//! Append-only JSONL, one file per kind, under `<root>/memory/`. The same convention as
//! `scematica-trades.jsonl` in the bot workspace, for the same reason: an append-only log
//! cannot lose an earlier belief when a later one contradicts it, and contradiction is
//! information. Nothing here rewrites or deletes a line.

use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Which memory a record belongs to. Determines the file it lands in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryKind {
    Episodic,
    Semantic,
    Procedural,
    Counterfactual,
}

impl MemoryKind {
    pub fn filename(&self) -> &'static str {
        match self {
            MemoryKind::Episodic => "episodic.jsonl",
            MemoryKind::Semantic => "semantic.jsonl",
            MemoryKind::Procedural => "procedural.jsonl",
            MemoryKind::Counterfactual => "counterfactual.jsonl",
        }
    }

    pub fn all() -> [MemoryKind; 4] {
        [
            MemoryKind::Episodic,
            MemoryKind::Semantic,
            MemoryKind::Procedural,
            MemoryKind::Counterfactual,
        ]
    }
}

/// How something went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Succeeded,
    Failed,
    /// Started and neither finished nor failed. A real state, and the one an agent is most
    /// tempted to round to `Failed`.
    Abandoned,
    /// Nobody checked. Distinct from `Abandoned`: the action may well have worked.
    Unobserved,
}

/// The content of a memory.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "body", rename_all = "snake_case")]
pub enum MemoryBody {
    /// Something that happened.
    Episode {
        what: String,
        outcome: Outcome,
        evidence: Vec<String>,
    },
    /// Something the agent holds to be true, with the tally that supports it.
    ///
    /// `support` and `contradiction` are both kept rather than folded into one confidence,
    /// because "believed on 40 observations, contradicted by 39" and "believed on 1
    /// observation" produce the same ratio and are not the same belief.
    Belief {
        claim: String,
        support: u32,
        contradiction: u32,
    },
    /// A way of doing something, and how it has gone.
    Procedure {
        name: String,
        steps: Vec<String>,
        successes: u32,
        failures: u32,
    },
    /// A branch that was considered and not taken.
    ///
    /// `projected` is what the simulator said. There is no `realised` field: an outcome for
    /// an untaken branch would have to be invented. Resolution, in the rare case the branch
    /// is later run, arrives as a separate [`MemoryBody::Realisation`].
    Counterfactual {
        decision: String,
        hypothesis: String,
        statement: String,
        projected: f64,
        /// Why it lost: outranked, forbidden, contested, or the whole decision abstained.
        reason: String,
    },
    /// A measured outcome for a branch, which may resolve a counterfactual.
    Realisation {
        decision: String,
        hypothesis: String,
        realised: f64,
        note: String,
    },
}

/// One line in one of the four logs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub id: String,
    pub kind: MemoryKind,
    /// Unix seconds.
    pub at: i64,
    /// What this is about — an entity locator, a decision id, a subsystem name. Recall
    /// filters on it.
    pub subject: String,
    pub tags: Vec<String>,
    pub body: MemoryBody,
    /// What produced this record: an observer name, a decision id, `operator`.
    pub source: String,
}

impl MemoryRecord {
    pub fn new(
        id: impl Into<String>,
        kind: MemoryKind,
        at: i64,
        subject: impl Into<String>,
        body: MemoryBody,
        source: impl Into<String>,
    ) -> Self {
        MemoryRecord {
            id: id.into(),
            kind,
            at,
            subject: subject.into(),
            tags: vec![],
            body,
            source: source.into(),
        }
    }

    pub fn tagged(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }
}

/// What to recall.
#[derive(Clone, Debug, Default)]
pub struct Recall {
    /// Case-insensitive substring of `subject`.
    pub subject: Option<String>,
    pub tag: Option<String>,
    /// Only records at or after this unix second.
    pub since: Option<i64>,
    /// Most recent first; `None` for all.
    pub limit: Option<usize>,
}

impl Recall {
    pub fn about(subject: impl Into<String>) -> Self {
        Recall { subject: Some(subject.into()), ..Default::default() }
    }

    pub fn limit(mut self, n: usize) -> Self {
        self.limit = Some(n);
        self
    }

    fn matches(&self, r: &MemoryRecord) -> bool {
        if let Some(s) = &self.subject {
            if !r.subject.to_lowercase().contains(&s.to_lowercase()) {
                return false;
            }
        }
        if let Some(t) = &self.tag {
            if !r.tags.iter().any(|x| x == t) {
                return false;
            }
        }
        if let Some(since) = self.since {
            if r.at < since {
                return false;
            }
        }
        true
    }
}

/// How well past projections matched reality — over the branches that were resolvable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Calibration {
    /// Counterfactuals recorded in total.
    pub recorded: usize,
    /// Those with a matching [`MemoryBody::Realisation`].
    pub resolved: usize,
    /// Those without. **Counted, never scored** — see the crate note.
    pub unresolved: usize,
    /// Mean absolute error over the resolved ones only, or `None` when none resolved.
    ///
    /// `None` rather than `0.0`: a perfect score and no evidence must not print alike.
    pub mean_abs_error: Option<f64>,
}

/// Make a state root ignore itself, the moment it first exists.
///
/// `.scema/` holds decision records full of absolute paths, four append-only memory logs,
/// and — when the daemon has run — a 256-bit pairing token. None of that is meaningful in
/// somebody else's clone, and the token is a secret sitting inside a git working tree.
///
/// The ignore is written **inside** the directory rather than into the project's own
/// `.gitignore`, for two reasons. A self-ignoring directory works whatever the project's
/// ignore rules say, and whatever VCS it uses; and no library has any business rewriting a
/// file the whole repository shares.
///
/// It is called from every place that can bring the root into existence — the record store,
/// the memory store, and the daemon's token write — because whichever of those runs *first*
/// is the one that creates it, and that varies by which surface the operator reached for.
/// `scema init` writes the same file, so an operator who set the directory up deliberately
/// and one who got it as a side effect end up with the same protection.
///
/// Failure is deliberately silent. This is a courtesy on the way to doing something else,
/// and an unwritable `.gitignore` must not turn a successful `decide` into an error — the
/// record is the thing the caller asked for. A pre-existing file is never overwritten,
/// because an operator who edited it meant it.
pub fn self_ignore(root: &Path) {
    let marker = root.join(".gitignore");
    if marker.exists() {
        return;
    }
    let _ = fs::write(
        &marker,
        "# Machine-local agent state: decision records cite absolute paths, memory is a\n         # per-checkout history, and omnid.token is a secret. None of it belongs in a commit.\n         *\n",
    );
}

/// The four logs on disk.
pub struct MemoryStore {
    root: PathBuf,
}

impl MemoryStore {
    /// Memory lives under `<root>/memory/`. Nothing is created until the first write.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        MemoryStore { root: root.into() }
    }

    fn dir(&self) -> PathBuf {
        self.root.join("memory")
    }

    fn path(&self, kind: MemoryKind) -> PathBuf {
        self.dir().join(kind.filename())
    }

    /// Append one record.
    ///
    /// Appends rather than the tmp-and-rename used for snapshot files: a single `write` of
    /// one line to a file opened with `append` is what the bot's trade log already relies
    /// on, and rewriting the whole log to add a line would lose history on a crash.
    pub fn remember(&self, record: &MemoryRecord) -> Result<()> {
        let dir = self.dir();
        fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        self_ignore(&self.root);
        let path = self.path(record.kind);
        let mut line = serde_json::to_string(record)?;
        line.push('\n');
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("opening {}", path.display()))?;
        f.write_all(line.as_bytes())
            .with_context(|| format!("appending to {}", path.display()))?;
        Ok(())
    }

    /// Every record of a kind, oldest first.
    ///
    /// A line that will not parse is **skipped and counted**, not fatal: one corrupt line —
    /// a half-written record from a killed process — must not make the agent amnesiac. The
    /// count comes back so a caller can surface it rather than swallow it.
    pub fn read_all(&self, kind: MemoryKind) -> Result<(Vec<MemoryRecord>, usize)> {
        let path = self.path(kind);
        if !path.exists() {
            return Ok((vec![], 0));
        }
        let f = fs::File::open(&path).with_context(|| format!("opening {}", path.display()))?;
        let mut out = Vec::new();
        let mut corrupt = 0usize;
        for line in BufReader::new(f).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<MemoryRecord>(&line) {
                Ok(r) => out.push(r),
                Err(_) => corrupt += 1,
            }
        }
        Ok((out, corrupt))
    }

    /// Matching records, most recent first.
    pub fn recall(&self, kind: MemoryKind, query: &Recall) -> Result<Vec<MemoryRecord>> {
        let (all, _) = self.read_all(kind)?;
        let mut hits: Vec<MemoryRecord> = all.into_iter().filter(|r| query.matches(r)).collect();
        hits.sort_by_key(|r| std::cmp::Reverse(r.at));
        if let Some(n) = query.limit {
            hits.truncate(n);
        }
        Ok(hits)
    }

    /// Score past projections against outcomes, honestly.
    ///
    /// Joins counterfactuals to realisations on `(decision, hypothesis)`. Everything that
    /// does not join is `unresolved` — which, for branches the agent declined to take, is
    /// the overwhelming majority and always will be.
    pub fn calibration(&self) -> Result<Calibration> {
        let (records, _) = self.read_all(MemoryKind::Counterfactual)?;
        let mut projected: Vec<(String, String, f64)> = Vec::new();
        let mut realised: Vec<(String, String, f64)> = Vec::new();
        for r in &records {
            match &r.body {
                MemoryBody::Counterfactual { decision, hypothesis, projected: p, .. } => {
                    projected.push((decision.clone(), hypothesis.clone(), *p))
                }
                MemoryBody::Realisation { decision, hypothesis, realised: v, .. } => {
                    realised.push((decision.clone(), hypothesis.clone(), *v))
                }
                _ => {}
            }
        }
        let mut errors: Vec<f64> = Vec::new();
        for (d, h, p) in &projected {
            if let Some((_, _, v)) = realised.iter().find(|(rd, rh, _)| rd == d && rh == h) {
                errors.push((p - v).abs());
            }
        }
        let recorded = projected.len();
        let resolved = errors.len();
        Ok(Calibration {
            recorded,
            resolved,
            unresolved: recorded - resolved,
            mean_abs_error: if errors.is_empty() {
                None
            } else {
                Some(errors.iter().sum::<f64>() / errors.len() as f64)
            },
        })
    }

    /// Total records per kind, for `scema remember --stats`.
    pub fn counts(&self) -> Result<Vec<(MemoryKind, usize, usize)>> {
        MemoryKind::all()
            .iter()
            .map(|k| {
                let (rs, corrupt) = self.read_all(*k)?;
                Ok((*k, rs.len(), corrupt))
            })
            .collect()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "scema-omni-mem-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn cf(store: &MemoryStore, id: &str, decision: &str, hypothesis: &str, projected: f64) {
        store
            .remember(&MemoryRecord::new(
                id,
                MemoryKind::Counterfactual,
                1,
                decision,
                MemoryBody::Counterfactual {
                    decision: decision.into(),
                    hypothesis: hypothesis.into(),
                    statement: "s".into(),
                    projected,
                    reason: "outranked".into(),
                },
                "test",
            ))
            .unwrap();
    }

    #[test]
    fn untaken_branches_are_counted_and_never_scored() {
        // The rule the crate exists to hold. Two rejected branches, no outcomes: the report
        // must say "2 unresolved" and refuse to produce an error figure.
        let dir = tmp();
        let s = MemoryStore::new(&dir);
        cf(&s, "m1", "d1", "h2", 0.31);
        cf(&s, "m2", "d1", "h3", 0.11);
        let c = s.calibration().unwrap();
        assert_eq!(c.recorded, 2);
        assert_eq!(c.resolved, 0);
        assert_eq!(c.unresolved, 2);
        assert_eq!(c.mean_abs_error, None, "no evidence must not print as perfect accuracy");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_realisation_resolves_exactly_its_own_branch() {
        let dir = tmp();
        let s = MemoryStore::new(&dir);
        cf(&s, "m1", "d1", "h2", 0.30);
        cf(&s, "m2", "d1", "h3", 0.10);
        s.remember(&MemoryRecord::new(
            "m3",
            MemoryKind::Counterfactual,
            2,
            "d1",
            MemoryBody::Realisation {
                decision: "d1".into(),
                hypothesis: "h2".into(),
                realised: 0.20,
                note: "ran it later".into(),
            },
            "test",
        ))
        .unwrap();
        let c = s.calibration().unwrap();
        assert_eq!(c.resolved, 1);
        assert_eq!(c.unresolved, 1);
        assert!((c.mean_abs_error.unwrap() - 0.10).abs() < 1e-9);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_realisation_for_a_different_decision_does_not_resolve_anything() {
        let dir = tmp();
        let s = MemoryStore::new(&dir);
        cf(&s, "m1", "d1", "h2", 0.30);
        s.remember(&MemoryRecord::new(
            "m2",
            MemoryKind::Counterfactual,
            2,
            "d9",
            MemoryBody::Realisation {
                decision: "d9".into(),
                hypothesis: "h2".into(),
                realised: 0.9,
                note: "different decision entirely".into(),
            },
            "test",
        ))
        .unwrap();
        assert_eq!(s.calibration().unwrap().resolved, 0);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_corrupt_line_is_skipped_and_counted_rather_than_fatal() {
        let dir = tmp();
        let s = MemoryStore::new(&dir);
        s.remember(&MemoryRecord::new(
            "m1",
            MemoryKind::Episodic,
            1,
            "x",
            MemoryBody::Episode {
                what: "did a thing".into(),
                outcome: Outcome::Succeeded,
                evidence: vec![],
            },
            "test",
        ))
        .unwrap();
        let path = dir.join("memory").join("episodic.jsonl");
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"{ this is not json\n").unwrap();

        let (records, corrupt) = s.read_all(MemoryKind::Episodic).unwrap();
        assert_eq!(records.len(), 1, "one bad line must not make the agent amnesiac");
        assert_eq!(corrupt, 1, "and it must not be swallowed either");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn recall_returns_most_recent_first() {
        let dir = tmp();
        let s = MemoryStore::new(&dir);
        for (id, at) in [("a", 10), ("b", 30), ("c", 20)] {
            s.remember(&MemoryRecord::new(
                id,
                MemoryKind::Semantic,
                at,
                "rpc",
                MemoryBody::Belief { claim: id.into(), support: 1, contradiction: 0 },
                "test",
            ))
            .unwrap();
        }
        let hits = s.recall(MemoryKind::Semantic, &Recall::about("rpc").limit(2)).unwrap();
        assert_eq!(hits.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["b", "c"]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reading_a_store_that_was_never_written_is_empty_not_an_error() {
        let s = MemoryStore::new(tmp().join("nope"));
        assert!(s.recall(MemoryKind::Episodic, &Recall::default()).unwrap().is_empty());
        assert_eq!(s.calibration().unwrap().recorded, 0);
    }
}
