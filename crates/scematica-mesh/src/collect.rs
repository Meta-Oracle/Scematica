//! Collection: turn the files the running system leaves on disk into a [`Mesh`].
//!
//! The sniper and its satellites communicate exclusively through JSON files in the working
//! directory (see the File-Based IPC table in CLAUDE.md). This module is a **reader only**.
//! It writes nothing, takes no locks, and treats every file as possibly missing, possibly
//! stale and possibly malformed — because it is observing processes that were not written
//! for its benefit and can be restarted underneath it mid-pass.
//!
//! ## Two rules that shape everything here
//!
//! **A missing file produces a dark node, never a zero.** This is the same rule the escrow
//! page enforces about reserves, for the same reason: `0` and `unknown` are different
//! claims, and only one of them is an accusation against the system being observed.
//!
//! **Freshness budgets are per source.** The sniper rewrites its metrics every 5 seconds;
//! the LLM strategy agent may not write for an hour; the deployer reputation ledger only
//! changes when a trade resolves. One shared staleness constant would either mark a
//! perfectly healthy reputation file stale forever or never notice a dead sniper. The
//! budgets below are derived from each writer's documented cadence and are marked as such
//! — they are declared, not measured, and saying so is cheaper than being quietly wrong.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::Value;

use crate::edge::Edge;
use crate::node::{analyse_veto, Node, NodeKind, Provenance, Verdict};
use crate::topology::Mesh;

/// A file this collector reads, and how long its contents stay meaningful.
#[derive(Clone, Copy, Debug)]
pub struct Source {
    pub path: &'static str,
    /// Age past which the contents are reported [`Provenance::Stale`].
    ///
    /// Set to several times the writer's documented interval, because a budget equal to
    /// the interval flickers stale on every ordinary scheduling jitter, and a status that
    /// flickers trains people to ignore it — the same reasoning as the 15% staleness
    /// tolerance on the Chainlink feeds in `alchem-link`.
    pub budget_secs: u64,
}

pub const METRICS: Source = Source { path: "scematica-metrics.json", budget_secs: 30 };
pub const FILTER_STATS: Source = Source { path: "scematica-filter-stats.json", budget_secs: 120 };
pub const POOL_SCORES: Source = Source { path: "scematica-pool-scores.json", budget_secs: 300 };
pub const NN_STATS: Source = Source { path: "scematica-nn-stats.json", budget_secs: 120 };
pub const NN_ADVICE: Source = Source { path: "scematica-nn-advice.json", budget_secs: 120 };
pub const NN_TOURNAMENT: Source = Source { path: "scematica-nn-tournament.json", budget_secs: 1800 };
pub const STRATEGY: Source = Source { path: "scematica-strategy.json", budget_secs: 3600 };
pub const REPUTATION: Source = Source { path: "scematica-deployer-reputation.json", budget_secs: 86_400 };
pub const POSITIONS: Source = Source { path: "scematica-positions.json", budget_secs: 120 };

/// A source that was read, or was not there.
#[derive(Clone, Debug)]
pub struct Reading {
    pub value: Option<Value>,
    pub provenance: Provenance,
}

impl Reading {
    fn absent() -> Self {
        Reading { value: None, provenance: Provenance::Absent }
    }

    /// Convenience accessor: `None` whenever the file was missing OR the key was absent,
    /// so a caller cannot accidentally treat "unreadable" as a value.
    fn f64(&self, key: &str) -> Option<f64> {
        self.value.as_ref()?.get(key)?.as_f64()
    }

    fn u64(&self, key: &str) -> Option<u64> {
        self.value.as_ref()?.get(key)?.as_u64()
    }

    fn bool(&self, key: &str) -> Option<bool> {
        self.value.as_ref()?.get(key)?.as_bool()
    }

    fn str(&self, key: &str) -> Option<String> {
        Some(self.value.as_ref()?.get(key)?.as_str()?.to_string())
    }
}

/// Reads a working directory and produces a mesh observation.
pub struct Collector {
    root: PathBuf,
    /// Injectable clock. Present so the staleness logic is testable without sleeping or
    /// touching file mtimes, which is the only way the stale/live boundary gets covered.
    now: SystemTime,
}

impl Collector {
    pub fn new(root: impl AsRef<Path>) -> Self {
        Collector { root: root.as_ref().to_path_buf(), now: SystemTime::now() }
    }

    /// Fix the clock, for tests.
    pub fn at(mut self, now: SystemTime) -> Self {
        self.now = now;
        self
    }

    /// Read one source, classifying it by measured file age.
    ///
    /// A file that exists but does not parse is reported [`Provenance::Absent`] rather
    /// than surfacing a partial value: half a JSON document is not a smaller truth, and
    /// the writer uses write-to-temp-then-rename precisely so a torn read should not
    /// happen — if one does, the honest report is that nothing was read.
    pub fn read(&self, source: Source) -> Reading {
        let path = self.root.join(source.path);
        let Ok(meta) = fs::metadata(&path) else { return Reading::absent() };
        let Ok(modified) = meta.modified() else { return Reading::absent() };
        let age = self.now.duration_since(modified).map(|d| d.as_secs()).unwrap_or(0);

        let Ok(text) = fs::read_to_string(&path) else { return Reading::absent() };
        let Ok(value) = serde_json::from_str::<Value>(&text) else { return Reading::absent() };

        let provenance = if age <= source.budget_secs {
            Provenance::Live { age_secs: age }
        } else {
            Provenance::Stale { age_secs: age, budget_secs: source.budget_secs }
        };
        Reading { value: Some(value), provenance }
    }

    /// Build the full observation.
    pub fn collect(&self) -> Mesh {
        let metrics = self.read(METRICS);
        let filters = self.read(FILTER_STATS);
        let scores = self.read(POOL_SCORES);
        let nn = self.read(NN_STATS);
        let advice = self.read(NN_ADVICE);
        let tournament = self.read(NN_TOURNAMENT);
        let strategy = self.read(STRATEGY);
        let reputation = self.read(REPUTATION);
        let positions = self.read(POSITIONS);

        let mut nodes = Vec::new();
        let mut edges = Vec::new();

        // ── ingest ────────────────────────────────────────────────────────────
        let seen = filters.f64("pools_seen");
        let passed = filters.f64("pools_passed");
        nodes.push(Node {
            id: "listener.pools".into(),
            kind: NodeKind::Listener,
            label: "Pool listener".into(),
            blurb: "Raydium / Pump.fun / whale-copy events merged into one stream".into(),
            provenance: filters.provenance.clone(),
            verdict: if seen.unwrap_or(0.0) > 0.0 { Verdict::Pass } else { Verdict::Idle },
            activity: None,
            detail: vec![
                ("pools seen".into(), fmt_opt(seen)),
                ("tracked".into(), fmt_opt(metrics.f64("pools_tracked"))),
                ("uptime".into(), metrics.u64("uptime_secs").map(fmt_dur).unwrap_or_else(dash)),
            ],
            reason: None,
        });

        // ── filter pipeline, plus one node per filter that has rejected anything ──
        //
        // A filter rejecting pools is throughput, not a fault: rejecting is the job. So
        // these get SIGNAL edges carrying a share label, and `Veto` edges stay reserved
        // for units that can halt the entire buy path. Collapsing the two would put the
        // page permanently in alarm during completely normal operation.
        let pass_rate = match (passed, seen) {
            (Some(p), Some(s)) if s > 0.0 => Some(p / s),
            _ => None,
        };
        nodes.push(Node {
            id: "filter.pipeline".into(),
            kind: NodeKind::Filter,
            label: "Filter pipeline".into(),
            blurb: "Every pool must clear each registered filter; RPC-bound ones fail open".into(),
            provenance: filters.provenance.clone(),
            verdict: Verdict::Pass,
            activity: pass_rate,
            detail: vec![
                ("passed".into(), fmt_opt(passed)),
                ("seen".into(), fmt_opt(seen)),
                ("pass rate".into(), pass_rate.map(fmt_pct).unwrap_or_else(dash)),
            ],
            reason: None,
        });
        edges.push(
            Edge::signal("listener.pools", "filter.pipeline")
                .with_label(match (passed, seen) {
                    (Some(p), Some(s)) => format!("{p:.0}/{s:.0} passed"),
                    _ => "—".to_string(),
                })
                .with_active(seen.map(|s| s > 0.0)),
        );

        if let Some(rejections) = filters.value.as_ref().and_then(|v| v.get("rejections")).and_then(|v| v.as_object()) {
            let total_seen = seen.unwrap_or(0.0);
            let mut named: Vec<(&String, f64)> = rejections
                .iter()
                .filter_map(|(k, v)| v.as_f64().map(|n| (k, n)))
                .collect();
            // Heaviest rejector first — that is the one an operator wants named.
            named.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            for (name, count) in named {
                let share = if total_seen > 0.0 { Some(count / total_seen) } else { None };
                let id = format!("filter.{name}");
                nodes.push(Node {
                    id: id.clone(),
                    kind: NodeKind::Filter,
                    label: name.clone(),
                    blurb: "one stage of the filter pipeline".into(),
                    provenance: filters.provenance.clone(),
                    verdict: if count > 0.0 { Verdict::Veto } else { Verdict::Pass },
                    activity: share,
                    detail: vec![
                        ("rejected".into(), format!("{count:.0}")),
                        ("share of seen".into(), share.map(fmt_pct).unwrap_or_else(dash)),
                    ],
                    reason: share.map(|s| format!("rejected {:.0} pools, {} of everything seen", count, fmt_pct(s))),
                });
                edges.push(
                    Edge::signal(&id, "filter.pipeline")
                        .with_label(format!("−{count:.0}"))
                        .with_active(Some(count > 0.0)),
                );
            }
        }

        // ── scorer ────────────────────────────────────────────────────────────
        let score_count = scores
            .value
            .as_ref()
            .and_then(|v| v.get("records"))
            .and_then(|v| v.as_object())
            .map(|m| m.len());
        nodes.push(Node {
            id: "scorer.pool".into(),
            kind: NodeKind::Scorer,
            label: "Pool scorer".into(),
            blurb: "0–100 predictive score from pool age and quote vault".into(),
            provenance: scores.provenance.clone(),
            verdict: if score_count.unwrap_or(0) > 0 { Verdict::Pass } else { Verdict::Idle },
            activity: None,
            detail: vec![("pools scored".into(), score_count.map(|n| n.to_string()).unwrap_or_else(dash))],
            reason: None,
        });
        edges.push(Edge::signal("filter.pipeline", "scorer.pool").with_active(Some(true)));

        // ── the DQ* learner, and the edge this whole feature exists for ───────
        let q_values: Vec<f64> = nn
            .value
            .as_ref()
            .and_then(|v| v.get("last_q_values"))
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_f64()).collect())
            .unwrap_or_default();
        let ready = nn.bool("ready_to_advise").unwrap_or(false);
        let veto = if q_values.is_empty() { None } else { Some(analyse_veto(&q_values, ready)) };

        let dq_visible = nn.provenance.is_visible();
        nodes.push(Node {
            id: "learner.dqstar".into(),
            kind: NodeKind::Learner,
            label: "DQ* agent".into(),
            blurb: "Dueling Double-DQN that sizes entries and can veto a buy outright".into(),
            provenance: nn.provenance.clone(),
            verdict: veto.as_ref().map(|v| v.verdict).unwrap_or(Verdict::Unknown),
            activity: nn.f64("epsilon").map(|e| 1.0 - e),
            detail: vec![
                ("train steps".into(), fmt_opt(nn.f64("train_steps"))),
                ("epsilon".into(), nn.f64("epsilon").map(|e| format!("{e:.3}")).unwrap_or_else(dash)),
                ("replay".into(), fmt_opt(nn.f64("replay_size"))),
                ("last action".into(), advice.str("action").or_else(|| nn.str("last_action")).unwrap_or_else(dash)),
                ("advising".into(), if ready { "yes".into() } else { "not yet".into() }),
                ("best buy Q".into(), veto.as_ref().map(|v| format!("{:.2}", v.buy_q)).unwrap_or_else(dash)),
                ("bearish Q".into(), veto.as_ref().map(|v| format!("{:.2}", v.bearish_q)).unwrap_or_else(dash)),
            ],
            reason: veto.as_ref().map(|v| v.reason.clone()),
        });
        edges.push(Edge::signal("scorer.pool", "learner.dqstar").with_active(Some(dq_visible)));

        // The veto edge. `active` is `None` when the agent could not be read, so an
        // unreadable DQ* renders as an unknown gate rather than an open one.
        let veto_active = if !dq_visible {
            None
        } else {
            Some(veto.as_ref().map(|v| v.verdict == Verdict::Veto).unwrap_or(false))
        };
        edges.push(
            Edge::veto("learner.dqstar", "exec.executor")
                .with_active(veto_active)
                .with_label("buy veto"),
        );

        // ── tournament variants ───────────────────────────────────────────────
        //
        // Absent on this machine, and that is worth seeing rather than hiding: the
        // tournament only writes its file once it has run, so three dark variants is an
        // accurate statement that the competition is not currently observable.
        let variant_names = tournament
            .value
            .as_ref()
            .and_then(|v| v.get("agent_names"))
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect::<Vec<_>>())
            .unwrap_or_else(|| vec!["conservative".into(), "balanced".into(), "aggressive".into()]);
        let rewards = tournament
            .value
            .as_ref()
            .and_then(|v| v.get("agent_total_rewards"))
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|x| x.as_f64()).collect::<Vec<_>>())
            .unwrap_or_default();
        let primary_idx = tournament.u64("primary_idx").map(|n| n as usize);

        for (i, name) in variant_names.iter().enumerate() {
            let id = format!("learner.variant.{name}");
            if tournament.provenance.is_visible() {
                nodes.push(Node {
                    id: id.clone(),
                    kind: NodeKind::Learner,
                    label: name.clone(),
                    blurb: "tournament variant, paper-trading in parallel".into(),
                    provenance: tournament.provenance.clone(),
                    verdict: if primary_idx == Some(i) { Verdict::Pass } else { Verdict::Idle },
                    activity: None,
                    detail: vec![
                        ("total reward".into(), rewards.get(i).map(|r| format!("{r:.1}")).unwrap_or_else(dash)),
                        ("primary".into(), if primary_idx == Some(i) { "yes".into() } else { "no".into() }),
                    ],
                    reason: None,
                });
            } else {
                nodes.push(Node::absent(
                    &id,
                    NodeKind::Learner,
                    name,
                    "tournament variant, paper-trading in parallel",
                ));
            }
            edges.push(
                Edge {
                    from: id,
                    to: "learner.dqstar".into(),
                    kind: crate::edge::EdgeKind::Promotion,
                    active: if tournament.provenance.is_visible() { Some(primary_idx == Some(i)) } else { None },
                    label: None,
                },
            );
        }

        // ── LLM strategy agent, and the Ψ gate over it ────────────────────────
        nodes.push(Node {
            id: "reasoner.strategy".into(),
            kind: NodeKind::Reasoner,
            label: "Strategy agent".into(),
            blurb: "LLM that proposes TP/SL and a position multiplier".into(),
            provenance: strategy.provenance.clone(),
            verdict: match strategy.provenance {
                Provenance::Live { .. } => Verdict::Pass,
                Provenance::Stale { .. } => Verdict::Degraded,
                _ => Verdict::Unknown,
            },
            activity: None,
            detail: vec![
                ("take profit".into(), strategy.f64("take_profit_pct").map(|v| format!("{v:.0}%")).unwrap_or_else(dash)),
                ("stop loss".into(), strategy.f64("stop_loss_pct").map(|v| format!("{v:.0}%")).unwrap_or_else(dash)),
                ("multiplier".into(), strategy.f64("amount_multiplier").map(|v| format!("{v:.2}x")).unwrap_or_else(dash)),
                ("regime".into(), strategy.str("market_regime").unwrap_or_else(dash)),
                ("written".into(), strategy.str("last_updated").unwrap_or_else(dash)),
            ],
            reason: match strategy.provenance {
                Provenance::Stale { age_secs, .. } => Some(format!(
                    "last wrote {} ago — the sniper is still applying these numbers, so they are live parameters from a dead author",
                    fmt_dur(age_secs)
                )),
                _ => None,
            },
        });
        edges.push(Edge::signal("reasoner.strategy", "exec.executor").with_label("TP/SL"));

        // The Ψ gate keeps no state file of its own — it is computed on demand by the API
        // from the same inputs. From this collector's position it is genuinely unseen, and
        // claiming otherwise would be inventing a node.
        nodes.push(Node::absent(
            "gate.psi",
            NodeKind::Gate,
            "Ψ gate",
            "data-integrity gate; HOLD stops the model being called at all",
        ));
        edges.push(Edge::gate("gate.psi", "reasoner.strategy"));

        // ── risk breakers ─────────────────────────────────────────────────────
        let rep_count = reputation
            .value
            .as_ref()
            .and_then(|v| v.get("records"))
            .and_then(|v| v.as_object())
            .map(|m| m.len());
        nodes.push(Node {
            id: "breaker.reputation".into(),
            kind: NodeKind::Breaker,
            label: "Deployer reputation".into(),
            blurb: "EMA-blended rug history per deployer".into(),
            provenance: reputation.provenance.clone(),
            verdict: if rep_count.unwrap_or(0) > 0 { Verdict::Pass } else { Verdict::Idle },
            activity: None,
            detail: vec![("deployers tracked".into(), rep_count.map(|n| n.to_string()).unwrap_or_else(dash))],
            reason: None,
        });
        edges.push(Edge::veto("breaker.reputation", "exec.executor").with_active(Some(false)));

        // The other five breakers write no state at all. Rendering them dark is the point:
        // it is an accurate report that five independent safety systems are running
        // unobserved, which is a finding rather than an omission.
        for (id, label, blurb) in [
            ("breaker.coherence", "Coherence (Ψ)", "halts buys when the pipeline is passing pools it could not verify"),
            ("breaker.kelly", "Kelly sizing", "fractional Kelly from rolling win-rate"),
            ("breaker.ath", "ATH drawdown", "pauses buys on a drop from session ATH"),
            ("breaker.grief", "Grief breaker", "5-minute sliding-window cumulative loss"),
            ("breaker.multi_rpc", "Multi-RPC failover", "latency-ranked round-robin across endpoints"),
        ] {
            nodes.push(Node::absent(id, NodeKind::Breaker, label, blurb));
            edges.push(Edge::veto(id, "exec.executor"));
        }

        // ── execution ─────────────────────────────────────────────────────────
        let attempted = metrics.f64("trades_attempted");
        let confirmed = metrics.f64("trades_confirmed");
        let open = positions.value.as_ref().and_then(|v| v.as_array()).map(|a| a.len());
        nodes.push(Node {
            id: "exec.executor".into(),
            kind: NodeKind::Executor,
            label: "Executor".into(),
            blurb: "Swap construction, WSOL ATA lifecycle, fee escalation".into(),
            provenance: metrics.provenance.clone(),
            verdict: match attempted {
                Some(n) if n > 0.0 => Verdict::Pass,
                Some(_) => Verdict::Idle,
                None => Verdict::Unknown,
            },
            activity: None,
            detail: vec![
                ("attempted".into(), fmt_opt(attempted)),
                ("confirmed".into(), fmt_opt(confirmed)),
                ("failed".into(), fmt_opt(metrics.f64("trades_failed"))),
                ("open positions".into(), open.map(|n| n.to_string()).unwrap_or_else(dash)),
                ("pnl (lamports)".into(), fmt_opt(metrics.f64("total_pnl_lamports"))),
            ],
            reason: match (attempted, seen) {
                (Some(a), Some(s)) if a == 0.0 && s > 0.0 => Some(format!(
                    "{s:.0} pools reached the pipeline and none became a trade"
                )),
                _ => None,
            },
        });

        // ── the numeric signals the agentic gate needs (§16, §20) ─────────────
        //
        // Recovered here rather than re-read inside `cognition`, so there is exactly one
        // pass over the files and the gate is evaluated against the same bytes the nodes
        // were built from. Two reads could disagree if the sniper rewrote a file between
        // them, and a gate that disagrees with the graph above it is worse than no gate.
        let signals = crate::cognition::Signals {
            q_values: if q_values.is_empty() { None } else { Some(q_values.clone()) },
            variant_rewards: if rewards.is_empty() { None } else { Some(rewards.clone()) },
            intelligence_ratio: nn
                .value
                .as_ref()
                .and_then(|v| v.get("equations"))
                .and_then(|v| v.get("intelligence_ratio"))
                .and_then(|v| v.as_f64()),
            trades_attempted: attempted,
            trades_failed: metrics.f64("trades_failed"),
            world_model_active: nn.bool("world_model").unwrap_or(false),
        };

        let generated_at = chrono::Utc::now().to_rfc3339();
        Mesh::with_signals(nodes, edges, generated_at, &signals)
    }
}

fn dash() -> String {
    "—".to_string()
}

fn fmt_opt(v: Option<f64>) -> String {
    v.map(|n| format!("{n:.0}")).unwrap_or_else(dash)
}

fn fmt_pct(v: f64) -> String {
    format!("{:.1}%", v * 100.0)
}

fn fmt_dur(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("scematica-mesh-test-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// An entirely empty directory must produce a full, well-formed mesh in which
    /// everything is dark — not an error, and emphatically not a set of zeroes.
    #[test]
    fn an_empty_directory_yields_a_dark_but_valid_mesh() {
        let d = tmpdir("empty");
        let mesh = Collector::new(&d).collect();
        assert!(mesh.validate().is_empty(), "{:?}", mesh.validate());
        assert_eq!(mesh.summary.nodes_live, 0);
        assert_eq!(mesh.summary.visibility, 0.0);
        assert!(mesh.summary.diagnosis.contains("nothing is visible"));
        for n in &mesh.nodes {
            assert!(n.activity.is_none() || n.provenance.is_visible());
        }
    }

    /// The captured production state: 474 pools seen, 171 passed, 0 trades attempted, and
    /// a Q-vector whose bearish action leads the best buy by more than 3x. The mesh must
    /// name the DQ* veto as the reason, because that is the actual answer to "why did
    /// nothing trade" and finding it by hand is the workflow this replaces.
    #[test]
    fn the_captured_production_state_diagnoses_the_dq_star_veto() {
        let d = tmpdir("veto");
        fs::write(
            d.join("scematica-filter-stats.json"),
            r#"{"pools_passed":171,"pools_seen":474,"rejections":{"PoolSize":278,"fibonacci_gate":42}}"#,
        )
        .unwrap();
        fs::write(
            d.join("scematica-metrics.json"),
            r#"{"trades_attempted":0,"trades_confirmed":0,"trades_failed":0,"pools_tracked":0,"total_pnl_lamports":0,"uptime_secs":3701}"#,
        )
        .unwrap();
        fs::write(
            d.join("scematica-nn-stats.json"),
            r#"{"epsilon":0.05,"train_steps":35964,"replay_size":1556,"ready_to_advise":true,
                "last_action":"SELL_PARTIAL",
                "last_q_values":[42.97557172797499,5.209099758311705,12.720168124042097,43.03030301,40.0]}"#,
        )
        .unwrap();

        let mesh = Collector::new(&d).collect();
        assert!(mesh.validate().is_empty(), "{:?}", mesh.validate());

        let dq = mesh.node("learner.dqstar").unwrap();
        assert_eq!(dq.verdict, Verdict::Veto);

        assert_eq!(mesh.summary.blocking, 1, "exactly one systemic veto is active");
        assert!(mesh.summary.diagnosis.contains("DQ*"), "got: {}", mesh.summary.diagnosis);

        // Filters rejecting pools is normal throughput and must NOT be counted as a
        // systemic block, or the page sits in permanent alarm during healthy operation.
        let pool_size = mesh.node("filter.PoolSize").unwrap();
        assert_eq!(pool_size.verdict, Verdict::Veto, "the node records that it rejects");
        assert!(
            !mesh.edges.iter().any(|e| e.from == "filter.PoolSize" && e.is_blocking()),
            "but its edge is throughput, not a halt"
        );

        // The executor explains its own silence rather than just showing a zero.
        let exec = mesh.node("exec.executor").unwrap();
        assert!(exec.reason.as_ref().unwrap().contains("474"));
    }

    /// A file older than its budget is stale, and stale must never read as current. The
    /// real `scematica-strategy.json` on this machine was written in May and the sniper is
    /// still applying its TP/SL — live parameters from a dead author, which is exactly the
    /// condition a shared staleness constant would have hidden.
    #[test]
    fn a_file_past_its_budget_is_stale_not_live() {
        let d = tmpdir("stale");
        fs::write(
            d.join("scematica-strategy.json"),
            r#"{"take_profit_pct":175.0,"stop_loss_pct":10.0,"amount_multiplier":1.33,"market_regime":"neutral"}"#,
        )
        .unwrap();

        // Read from far in the future: the file is real, but ancient.
        let later = SystemTime::now() + Duration::from_secs(STRATEGY.budget_secs + 10_000);
        let mesh = Collector::new(&d).at(later).collect();

        let s = mesh.node("reasoner.strategy").unwrap();
        assert!(matches!(s.provenance, Provenance::Stale { .. }));
        assert!(!s.provenance.is_actionable());
        assert_eq!(s.verdict, Verdict::Degraded);
        assert!(s.reason.as_ref().unwrap().contains("dead author"));
        assert_eq!(mesh.summary.nodes_stale, 1);
        assert_eq!(mesh.summary.nodes_live, 0);
    }

    /// Malformed JSON is absent, not a partial reading.
    #[test]
    fn a_torn_file_reads_as_absent() {
        let d = tmpdir("torn");
        fs::write(d.join("scematica-nn-stats.json"), "{\"epsilon\":0.05,").unwrap();
        let mesh = Collector::new(&d).collect();
        let dq = mesh.node("learner.dqstar").unwrap();
        assert_eq!(dq.provenance, Provenance::Absent);
        assert_eq!(dq.verdict, Verdict::Unknown);
    }

    /// An unreadable DQ* leaves its veto edge unknown. Rendering that as "not vetoing"
    /// would tell an operator a gate is open when nobody has looked at it.
    #[test]
    fn an_unreadable_agent_leaves_its_veto_edge_unknown() {
        let d = tmpdir("unknown-veto");
        let mesh = Collector::new(&d).collect();
        let e = mesh
            .edges
            .iter()
            .find(|e| e.from == "learner.dqstar" && e.to == "exec.executor")
            .unwrap();
        assert_eq!(e.active, None);
        assert!(!e.is_blocking());
    }

    /// The tournament file is absent on this machine; three dark variants is the correct
    /// report, and they must still be wired so the topology does not change shape when
    /// the file appears.
    #[test]
    fn absent_tournament_still_produces_wired_dark_variants() {
        let d = tmpdir("tournament");
        let mesh = Collector::new(&d).collect();
        for name in ["conservative", "balanced", "aggressive"] {
            let n = mesh.node(&format!("learner.variant.{name}")).unwrap();
            assert_eq!(n.provenance, Provenance::Absent);
        }
        assert!(mesh.validate().is_empty());
    }

    #[test]
    fn every_edge_endpoint_exists_on_a_fully_populated_read() {
        let d = tmpdir("full");
        fs::write(d.join("scematica-filter-stats.json"), r#"{"pools_passed":1,"pools_seen":2,"rejections":{"A":1}}"#).unwrap();
        fs::write(d.join("scematica-metrics.json"), r#"{"trades_attempted":5,"trades_confirmed":4,"trades_failed":1,"uptime_secs":10}"#).unwrap();
        fs::write(d.join("scematica-nn-stats.json"), r#"{"epsilon":0.1,"ready_to_advise":true,"last_q_values":[1.0,9.0,8.0,1.0,1.0]}"#).unwrap();
        fs::write(d.join("scematica-nn-tournament.json"), r#"{"primary_idx":1,"agent_names":["a","b"],"agent_total_rewards":[1.0,2.0]}"#).unwrap();
        fs::write(d.join("scematica-positions.json"), "[]").unwrap();
        let mesh = Collector::new(&d).collect();
        assert!(mesh.validate().is_empty(), "{:?}", mesh.validate());
        assert_eq!(mesh.node("learner.dqstar").unwrap().verdict, Verdict::Pass);
        assert_eq!(mesh.summary.blocking, 0);
        assert_eq!(mesh.node("learner.variant.b").unwrap().verdict, Verdict::Pass, "primary_idx 1 is b");
    }
}
