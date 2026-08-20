//! The bot's Deep Q* agent as **one evaluator among several**.
//!
//! This module is the concrete answer to a temptation the rest of this workspace is built
//! to resist. `scematica-nn` is a trained Dueling Double-DQN with a real edge on Raydium
//! pools, and it would be easy to declare it "the policy" and route every decision through
//! it. That would be wrong in a specific, checkable way: its 24-dimensional state is pool
//! age, liquidity, buy/sell ratio and position PnL. Asked to rank a refactor of a filter
//! pipeline it will still emit five finite Q-values, correctly shaped and entirely
//! meaningless.
//!
//! So it is wired in behind [`Applicability`], and on any world that is not
//! [`Domain::Trading`] it declines. The general utility equation stays authoritative; this
//! is a second opinion that applies to a slice of worlds.
//!
//! ## What it needs before it will speak
//!
//! Four things, each of which produces a *different* refusal:
//!
//! 1. A trading world. Otherwise [`Applicability::OutOfDomain`] — permanent, not a problem
//!    to fix.
//! 2. A checkpoint that loaded. Otherwise [`Applicability::Insufficient`] naming the path.
//! 3. `ready_to_advise()` — the agent's own bar (10k training steps plus signal in the last
//!    Q-vector). An undertrained net that votes is worse than one that does not.
//! 4. A complete [`TradeState`] in the world, under an object of kind `trade_state`. A
//!    partial state is refused rather than defaulted: a missing `initial_liquidity_sol`
//!    silently becomes `0.0`, which the net reads as a real observation of an empty pool.
//!
//! ## Dispersion, not magnitude
//!
//! The Q-values are unbounded and their absolute scale means nothing across checkpoints, so
//! the utility here is each action's position within the current spread. When the spread is
//! flat the net has no opinion, and this module says so with an unmeasured term rather than
//! returning `0.5` — value dispersion and action dispersion are different things, and a net
//! whose values differ while its preferences do not has told you nothing.

use scema_sim::Projection;
use scema_world::{Domain, Goal, Hypothesis, Scalar, Term, WorldState};
use scematica_nn::{DQNAgent, TradeAction, TradeState};

use crate::evaluator::{Applicability, Evaluation, Evaluator};

/// Object kind the world must carry for this evaluator to have an input.
pub const TRADE_STATE_KIND: &str = "trade_state";

/// Tag a hypothesis must set to say which trade it is.
pub const ACTION_TAG: &str = "dqstar.action";

/// The Deep Q* agent, wrapped as an [`Evaluator`].
pub struct DqStarEvaluator {
    agent: Option<DQNAgent>,
    checkpoint: String,
    /// Why the checkpoint did not load, verbatim. Kept so the refusal can name the real
    /// cause rather than "no agent".
    load_error: Option<String>,
}

impl DqStarEvaluator {
    /// Load a checkpoint written by the sniper (`scematica-nn-agent.json`).
    ///
    /// A missing or corrupt checkpoint is not an error here — it is a state this evaluator
    /// is expected to report through [`Applicability::Insufficient`], because an agent
    /// runtime that refuses to start when one optional specialist is unavailable is worse
    /// than one that runs without it and says so.
    pub fn from_checkpoint(path: impl Into<String>) -> Self {
        let checkpoint = path.into();
        match DQNAgent::load(&checkpoint) {
            Ok(agent) => DqStarEvaluator { agent: Some(agent), checkpoint, load_error: None },
            Err(e) => DqStarEvaluator {
                agent: None,
                checkpoint,
                load_error: Some(e.to_string()),
            },
        }
    }

    /// An evaluator with no checkpoint at all, for tests and for `scema policy` on a
    /// machine that has never run the bot.
    pub fn unloaded() -> Self {
        DqStarEvaluator {
            agent: None,
            checkpoint: String::new(),
            load_error: Some("no checkpoint configured".into()),
        }
    }

    /// Rebuild a [`TradeState`] from an object in the world.
    ///
    /// Strict by design: every field must be present. `serde`'s error is returned verbatim
    /// so the refusal names the missing feature.
    fn trade_state(world: &WorldState) -> Result<TradeState, String> {
        let obj = world
            .objects
            .iter()
            .find(|o| o.kind == TRADE_STATE_KIND)
            .ok_or_else(|| format!("world carries no object of kind `{TRADE_STATE_KIND}`"))?;

        if !obj.provenance.is_actionable() {
            return Err(format!(
                "the `{TRADE_STATE_KIND}` object is {} — a state that was true earlier is not a state to trade on",
                obj.provenance.label()
            ));
        }

        let mut map = serde_json::Map::new();
        for (k, v) in &obj.attrs {
            let json = match v {
                Scalar::Int(i) => serde_json::json!(i),
                Scalar::Num(n) => serde_json::json!(n),
                Scalar::Text(s) => serde_json::json!(s),
                Scalar::Bool(b) => serde_json::json!(b),
            };
            map.insert(k.clone(), json);
        }
        serde_json::from_value::<TradeState>(serde_json::Value::Object(map))
            .map_err(|e| format!("incomplete trade state: {e}"))
    }

    fn parse_action(tag: &str) -> Option<TradeAction> {
        match tag {
            "Hold" | "hold" => Some(TradeAction::Hold),
            "BuyStandard" | "buy" | "buy_standard" => Some(TradeAction::BuyStandard),
            "BuyAggressive" | "buy_aggressive" => Some(TradeAction::BuyAggressive),
            "SellPartial" | "sell_partial" => Some(TradeAction::SellPartial),
            "SellAll" | "sell" | "sell_all" => Some(TradeAction::SellAll),
            _ => None,
        }
    }
}

impl Evaluator for DqStarEvaluator {
    fn name(&self) -> &str {
        "dqstar"
    }

    fn about(&self) -> &str {
        "Scematica Deep Q* (Dueling Double-DQN) — ranks trading branches by their position in the current Q spread. Trading worlds only."
    }

    fn applicability(&self, world: &WorldState, _goal: &Goal) -> Applicability {
        if world.domain != Domain::Trading {
            return Applicability::OutOfDomain {
                note: format!(
                    "world domain is {:?}; the 24-feature state is pool and position data and has no reading of this",
                    world.domain
                ),
            };
        }
        let Some(agent) = &self.agent else {
            return Applicability::Insufficient {
                note: format!(
                    "no agent: {} (checkpoint `{}`)",
                    self.load_error.as_deref().unwrap_or("unknown"),
                    self.checkpoint
                ),
            };
        };
        if !agent.ready_to_advise() {
            let stats = agent.stats();
            return Applicability::Insufficient {
                note: format!(
                    "agent has not reached its own advisory bar ({} training steps); an undertrained net that votes is worse than one that abstains",
                    stats.train_steps
                ),
            };
        }
        match Self::trade_state(world) {
            Err(e) => Applicability::Insufficient { note: e },
            Ok(_) => Applicability::Applicable {
                note: format!("checkpoint `{}` loaded and a complete trade state is present", self.checkpoint),
            },
        }
    }

    fn score(
        &self,
        world: &WorldState,
        _goal: &Goal,
        hypothesis: &Hypothesis,
        _projection: &Projection,
    ) -> Option<Evaluation> {
        let agent = self.agent.as_ref()?;
        let state = Self::trade_state(world).ok()?;

        let tag = hypothesis.tags.get(ACTION_TAG)?;
        let action = Self::parse_action(tag)?;

        let (_greedy, q) = agent.greedy_action(&state);
        let idx = action.index();
        let q_a = *q.get(idx)?;

        let max = q.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min = q.iter().cloned().fold(f64::INFINITY, f64::min);
        let spread = max - min;

        // A flat spread means the net ranks every action alike. Reporting 0.5 here would
        // put "no opinion" and "genuinely middling" on the same footing, which is exactly
        // the confusion that once made a veto look like a signal.
        if spread <= f64::EPSILON {
            return Some(Evaluation {
                evaluator: self.name().to_string(),
                utility: Term::absent(
                    "Q",
                    "normalised Q",
                    0.0,
                    "Q-values are flat across all five actions; the net expresses no preference",
                ),
                confidence: Term::measured("d", "action dispersion", 0.0, "spread is zero"),
                note: format!("no preference between actions for `{tag}`"),
            });
        }

        let normalised = (q_a - min) / spread;
        // Dispersion relative to the mean magnitude: a real measurement of how much the
        // net discriminates on this input, independent of the absolute Q scale.
        let mean_abs = q.iter().map(|v| v.abs()).sum::<f64>() / q.len() as f64;
        let dispersion = if mean_abs > f64::EPSILON { (spread / mean_abs).min(1.0) } else { 0.0 };

        Some(Evaluation {
            evaluator: self.name().to_string(),
            utility: Term::measured(
                "Q",
                "normalised Q",
                // Re-centred to [-1, 1] so a bottom-ranked action reads as a negative
                // opinion rather than a small positive one. `decide` treats a measured
                // negative from a specialist as a contest, and "worst of five" should
                // contest.
                2.0 * normalised - 1.0,
                format!("Q({tag}) = {q_a:.4} within spread [{min:.4}, {max:.4}]"),
            ),
            confidence: Term::measured(
                "d",
                "action dispersion",
                dispersion,
                format!("spread {spread:.4} over mean |Q| {mean_abs:.4}"),
            ),
            note: format!("Q-vector {q:?}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use scema_world::{Entity, EntityKind, Extent, Object, Provenance};

    fn world(domain: Domain, objects: Vec<Object>) -> WorldState {
        WorldState {
            observer: "t".into(),
            entity: Entity { kind: EntityKind::Market, locator: "m".into(), label: "m".into() },
            domain,
            observed_at: 0,
            objects,
            facts: vec![],
            signals: vec![],
            extent: Extent::complete(0, "t"),
            blind_spots: vec![],
        }
    }

    #[test]
    fn a_software_world_is_out_of_domain_not_merely_unscored() {
        // The distinction the whole module exists for: this is permanent and correct, and
        // it must not read like a missing file the operator could go and supply.
        let e = DqStarEvaluator::unloaded();
        let a = e.applicability(&world(Domain::Software, vec![]), &Goal::new("g", "x"));
        assert!(matches!(a, Applicability::OutOfDomain { .. }));
        assert!(a.note().contains("pool and position"));
    }

    #[test]
    fn a_trading_world_without_a_checkpoint_is_insufficient_and_names_the_path() {
        let e = DqStarEvaluator::from_checkpoint("does-not-exist.json");
        let a = e.applicability(&world(Domain::Trading, vec![]), &Goal::new("g", "x"));
        assert!(matches!(a, Applicability::Insufficient { .. }));
        assert!(a.note().contains("does-not-exist.json"));
    }

    #[test]
    fn a_partial_trade_state_is_refused_rather_than_defaulted() {
        // Defaulting a missing feature to 0.0 hands the net a real-looking observation of
        // an empty pool. The refusal must name the field.
        let obj = Object::new("s", TRADE_STATE_KIND, "state", Provenance::Live { age_secs: 1 })
            .with("pool_age_secs", Scalar::Num(30.0));
        let err = DqStarEvaluator::trade_state(&world(Domain::Trading, vec![obj])).unwrap_err();
        assert!(err.contains("incomplete trade state"), "got {err}");
    }

    #[test]
    fn a_stale_trade_state_is_refused() {
        let obj = Object::new(
            "s",
            TRADE_STATE_KIND,
            "state",
            Provenance::Stale { age_secs: 900, budget_secs: 5 },
        );
        let err = DqStarEvaluator::trade_state(&world(Domain::Trading, vec![obj])).unwrap_err();
        assert!(err.contains("STALE"), "got {err}");
    }

    #[test]
    fn an_absent_trade_state_names_the_object_kind_that_is_missing() {
        let err = DqStarEvaluator::trade_state(&world(Domain::Trading, vec![])).unwrap_err();
        assert!(err.contains(TRADE_STATE_KIND));
    }
}
