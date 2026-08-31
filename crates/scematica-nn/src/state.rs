use serde::{Deserialize, Serialize};

/// Number of features in the state vector fed to the Q-network.
/// v1.1.0: expanded from 18 → 24 with peak PnL, pool quality, deployer rug rate,
/// volume velocity, price velocity, and price acceleration.
pub const STATE_DIM: usize = 24;

/// Names of the state features, **in the order `to_vec()` emits them**.
///
/// Exists so an exported model can describe its own input. A bare `[batch, 24]` tensor
/// is unusable to anyone without this file open, and a consumer that swaps two features
/// gets a confidently wrong policy and no error — the network happily evaluates
/// nonsense. `onnx::describe` writes these into the model's metadata.
///
/// Changing `to_vec()` without changing this list is the failure worth guarding, so
/// `state::tests::feature_names_match_vector_order` asserts the lengths agree.
pub const STATE_FEATURES: [&str; STATE_DIM] = [
    "pool_age_secs",
    "initial_liquidity_sol",
    "price_change_pct",
    "volume_5min_sol",
    "buy_sell_ratio",
    "lp_burned",
    "mint_renounced",
    "current_pnl_pct",
    "position_age_secs",
    "daily_pnl_sol",
    "consecutive_wins",
    "consecutive_losses",
    "sol_balance_sol",
    "regime",
    "volatility",
    "spread_pct",
    "time_of_day_norm",
    "open_positions",
    "peak_pnl_pct",
    "pool_score_norm",
    "deployer_rug_rate",
    "volume_velocity",
    "price_velocity",
    "price_acceleration",
];

// ── what the network is told it does not know ────────────────────────────────
//
// `to_vec()` normalises every field into [0, 1] and hands the result to a net that has no
// way to ask a follow-up question. A field nobody measured therefore arrives as *some*
// number, and the number it arrives as is a claim.
//
// That claim was found to be actively misleading. `measure` reports `pool_age_secs` as
// non-zero in 0 of 8,422 decisions — `pool.open_time` is essentially never populated — and
// `pool_age_secs: 0.0` normalises to `0.0`, the bottom of the range, which reads as **a
// pool zero seconds old**: the most bullish value the feature can take. The entry builder
// was also asserting `lp_burned: true` and `mint_renounced: true` outright, which are the
// two safest readings of the two strongest safety features.
//
// So the same rule the rest of this repository applies to renderers applies here, one layer
// down: an unmeasured dimension takes the **neutral element**, and never zero unless zero
// is what neutral means for that feature. `deployer_rug_rate: 0.5` in the sniper was
// already doing this, once, by hand.
//
// It cannot be a blanket 0.5, which is the trap worth naming. Two of these encodings are
// asymmetric: `price_change_pct` is `clamp(-1, 3) / 3`, so a 0% change sits at `0.0` and the
// midpoint of the range is **+150%**; `buy_sell_ratio` is `/ 5.0`, so a balanced book sits
// at `0.2`. A uniform midpoint would replace "I do not know" with "strongly bullish" on
// exactly the two features where that is most expensive.

/// The value each feature takes, in normalised space, when nothing measured it.
///
/// Indexed the same as [`STATE_FEATURES`] and [`TradeState::to_vec`]. Every entry is a
/// deliberate statement about what "no information" looks like for that feature, which is
/// why the ones that are not `0.5` carry a reason.
pub const NEUTRAL: [f64; STATE_DIM] = [
    0.5, // pool_age_secs — midpoint of the 0..1h band
    0.5, // initial_liquidity_sol
    0.0, // price_change_pct — the encoding puts a 0% change at 0.0, not at the midpoint
    0.5, // volume_5min_sol
    0.2, // buy_sell_ratio — a balanced book is 1.0, which the /5 encoding puts at 0.2
    0.5, // lp_burned — between the two truths, not either of them
    0.5, // mint_renounced
    0.5, // current_pnl_pct — the encoding already centres 0% at 0.5
    0.5, // position_age_secs
    0.5, // daily_pnl_sol — centred at 0.5
    0.0, // consecutive_wins — no streak is a real zero, and an unknown streak is no streak
    0.0, // consecutive_losses
    0.5, // sol_balance_sol
    0.5, // regime — sideways, which the (r+1)/2 encoding puts at 0.5
    0.5, // volatility
    0.5, // spread_pct
    0.5, // time_of_day_norm
    0.0, // open_positions — none
    0.0, // peak_pnl_pct — no peak yet
    0.5, // pool_score_norm
    0.5, // deployer_rug_rate
    0.5, // volume_velocity — centred at 0.5
    0.5, // price_velocity
    0.5, // price_acceleration
];

/// Which features of a [`TradeState`] nobody actually measured.
///
/// A bitset rather than 24 booleans so it costs one `u32` in a struct that is cloned per
/// pool, and so `Default` — which is what every existing construction site gets — means
/// **nothing is marked unmeasured**. That preserves the behaviour of every caller written
/// before this existed, which matters because the alternative default silently rewrites
/// every historical state into neutrals.
///
/// The direction of that default is a deliberate trade and it is the weaker half of this
/// design: a producer that does not know it should be marking a feature keeps lying by
/// omission. What makes it acceptable is [`TradeState::coverage`], which is recorded per
/// decision — so a producer that never marks anything shows up as claiming 100% coverage,
/// and a claim is checkable in a way that a silence is not.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FeatureMask(pub u32);

impl FeatureMask {
    /// Nothing is unmeasured.
    pub const NONE: Self = Self(0);

    /// Mark one feature, by its index in [`STATE_FEATURES`].
    pub fn mark(&mut self, index: usize) {
        if index < STATE_DIM {
            self.0 |= 1 << index;
        }
    }

    /// Mark one feature by name. Unknown names are ignored rather than panicking: this is
    /// reached from a live buy path, and a typo must not take the process down.
    pub fn mark_named(&mut self, name: &str) {
        if let Some(i) = STATE_FEATURES.iter().position(|f| *f == name) {
            self.mark(i);
        }
    }

    /// Builder form, for constructing a mask inline.
    pub fn with(mut self, name: &str) -> Self {
        self.mark_named(name);
        self
    }

    pub fn is_unmeasured(&self, index: usize) -> bool {
        index < STATE_DIM && self.0 & (1 << index) != 0
    }

    /// How many features are marked unmeasured.
    pub fn count(&self) -> usize {
        self.0.count_ones() as usize
    }

    /// The names, for a log line or a decision record.
    pub fn names(&self) -> Vec<&'static str> {
        STATE_FEATURES
            .iter()
            .enumerate()
            .filter(|(i, _)| self.is_unmeasured(*i))
            .map(|(_, n)| *n)
            .collect()
    }
}

/// Market + position context captured at decision time.
/// All numeric fields use real-world units; `to_vec()` normalises them to [0, 1].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TradeState {
    /// Which of the fields below nobody actually measured.
    ///
    /// `#[serde(default)]` so every state written before this existed still deserialises,
    /// and so a checkpoint's replay buffer keeps loading. Default is "everything measured",
    /// which is what those states were implicitly claiming anyway — see [`FeatureMask`].
    #[serde(default)]
    pub unmeasured: FeatureMask,
    /// How long the pool has been live (seconds).
    pub pool_age_secs: f64,
    /// SOL liquidity deposited at pool creation.
    pub initial_liquidity_sol: f64,
    /// Price change since pool creation (fractional, e.g. 0.5 = +50%).
    pub price_change_pct: f64,
    /// SOL volume traded in the last 5 minutes.
    pub volume_5min_sol: f64,
    /// Ratio of buy txs to sell txs in recent window.
    pub buy_sell_ratio: f64,
    /// LP tokens burned — strong safety signal.
    pub lp_burned: bool,
    /// Mint authority renounced — token cannot be inflated.
    pub mint_renounced: bool,
    /// Unrealised PnL on the current position (fractional).
    pub current_pnl_pct: f64,
    /// How long the current position has been open (seconds).
    pub position_age_secs: f64,
    /// Cumulative PnL for the day in SOL (can be negative).
    pub daily_pnl_sol: f64,
    /// Win streak length (positive) at this point in time.
    pub consecutive_wins: i32,
    /// Loss streak length (positive) at this point in time.
    pub consecutive_losses: i32,
    /// Current wallet SOL balance.
    pub sol_balance_sol: f64,
    /// Market regime: -1 bear, 0 neutral, 1 bull.
    pub regime: i32,
    /// Recent price volatility (std-dev / mean, unitless).
    pub volatility: f64,
    /// Bid-ask spread as a fraction of price.
    pub spread_pct: f64,
    /// UTC hour normalised to [0, 1].
    pub time_of_day_norm: f64,
    /// Number of open positions.
    pub open_positions: i32,

    // ── v1.1.0 new features ───────────────────────────────────────────────────

    /// Highest PnL seen since position entry (fractional, e.g. 0.8 = 80% peak).
    /// Enables the agent to reason about exit efficiency: how far off peak is the
    /// current exit?  0.0 if no position is open.
    pub peak_pnl_pct: f64,

    /// Pool predictive quality score normalised to [0, 1] (raw 0–100).
    /// Lets the agent learn to be more/less aggressive based on pool quality.
    pub pool_score_norm: f64,

    /// EMA deployer rug rate from the reputation ledger, [0, 1].
    /// 0 = no rugs recorded, 1 = all rugs. Defaults to 0.5 if unknown.
    pub deployer_rug_rate: f64,

    /// Rate of change of volume_5min_sol between the last two observations.
    /// Positive = volume growing (pump phase), negative = drying up (dump risk).
    /// Normalised: raw delta / 20 SOL, clamped to [-1, 1].
    pub volume_velocity: f64,

    /// First derivative of price_change_pct between consecutive observations.
    /// Positive = accelerating upward, negative = decelerating / reversing.
    /// Clamped to [-1, 1].
    pub price_velocity: f64,

    /// Second derivative of price_change_pct (change in velocity).
    /// Positive = still accelerating, negative = inflection point (momentum fading).
    /// Clamped to [-1, 1].
    pub price_acceleration: f64,
}

impl TradeState {
    /// Returns a normalised `[0, 1]` vector of length `STATE_DIM`.
    pub fn to_vec(&self) -> Vec<f64> {
        let raw = self.to_vec_raw();
        if self.unmeasured == FeatureMask::NONE {
            return raw;
        }
        raw.into_iter()
            .enumerate()
            .map(|(i, v)| if self.unmeasured.is_unmeasured(i) { NEUTRAL[i] } else { v })
            .collect()
    }

    /// How many features carried a measurement, as a fraction.
    ///
    /// Recorded per decision rather than only logged. A net given a vector that is one third
    /// invention still emits five finite, confidently-shaped Q-values — there is no channel
    /// in the output that says how much of the input was real, so the channel has to be
    /// beside it.
    pub fn coverage(&self) -> f64 {
        (STATE_DIM - self.unmeasured.count()) as f64 / STATE_DIM as f64
    }

    /// The normalised vector before neutral substitution.
    ///
    /// Kept separate so a test can show what the substitution changed, and so the encoding
    /// stays one function rather than two that drift.
    pub fn to_vec_raw(&self) -> Vec<f64> {
        vec![
            (self.pool_age_secs / 3_600.0).min(1.0),
            (self.initial_liquidity_sol / 100.0).min(1.0),
            self.price_change_pct.clamp(-1.0, 3.0) / 3.0,
            (self.volume_5min_sol / 50.0).min(1.0),
            (self.buy_sell_ratio / 5.0).min(1.0),
            if self.lp_burned { 1.0 } else { 0.0 },
            if self.mint_renounced { 1.0 } else { 0.0 },
            self.current_pnl_pct.clamp(-1.0, 2.0) / 2.0 + 0.5,
            (self.position_age_secs / 3_600.0).min(1.0),
            self.daily_pnl_sol.clamp(-2.0, 2.0) / 2.0 + 0.5,
            (self.consecutive_wins as f64 / 10.0).min(1.0),
            (self.consecutive_losses as f64 / 10.0).min(1.0),
            (self.sol_balance_sol / 10.0).min(1.0),
            (self.regime as f64 + 1.0) / 2.0,
            self.volatility.clamp(0.0, 1.0),
            (self.spread_pct / 0.1).min(1.0),
            self.time_of_day_norm.clamp(0.0, 1.0),
            (self.open_positions as f64 / 5.0).min(1.0),
            // v1.1.0 features
            self.peak_pnl_pct.clamp(0.0, 5.0) / 5.0,
            self.pool_score_norm.clamp(0.0, 1.0),
            self.deployer_rug_rate.clamp(0.0, 1.0),
            self.volume_velocity.clamp(-1.0, 1.0) * 0.5 + 0.5,
            self.price_velocity.clamp(-1.0, 1.0) * 0.5 + 0.5,
            self.price_acceleration.clamp(-1.0, 1.0) * 0.5 + 0.5,
        ]
    }

    /// Build a state from flat data available in scematica-trades.jsonl + metrics snapshot.
    /// New v1.1.0 fields default to zero / neutral when not provided by the replay loop.
    pub fn from_trade_fields(
        pnl_pct: f64,
        position_age_secs: f64,
        daily_pnl_sol: f64,
        consecutive_wins: i32,
        consecutive_losses: i32,
        sol_balance_sol: f64,
        open_positions: i32,
    ) -> Self {
        use chrono::Timelike;
        let hour = chrono::Utc::now().hour() as f64;
        Self {
            current_pnl_pct: pnl_pct,
            position_age_secs,
            daily_pnl_sol,
            consecutive_wins,
            consecutive_losses,
            sol_balance_sol,
            open_positions,
            time_of_day_norm: hour / 24.0,
            deployer_rug_rate: 0.5, // neutral unknown
            ..Default::default()
        }
    }

    /// Build a rich state during live trading with all v1.1.0 fields populated.
    pub fn from_live_fields(
        pnl_pct: f64,
        peak_pnl_pct: f64,
        position_age_secs: f64,
        daily_pnl_sol: f64,
        consecutive_wins: i32,
        consecutive_losses: i32,
        sol_balance_sol: f64,
        open_positions: i32,
        pool_score_norm: f64,
        deployer_rug_rate: f64,
        volume_velocity: f64,
        price_velocity: f64,
        price_acceleration: f64,
    ) -> Self {
        use chrono::Timelike;
        let hour = chrono::Utc::now().hour() as f64;
        Self {
            current_pnl_pct: pnl_pct,
            peak_pnl_pct,
            position_age_secs,
            daily_pnl_sol,
            consecutive_wins,
            consecutive_losses,
            sol_balance_sol,
            open_positions,
            pool_score_norm,
            deployer_rug_rate,
            volume_velocity,
            price_velocity,
            price_acceleration,
            time_of_day_norm: hour / 24.0,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── unmeasured features ──────────────────────────────────────────────────

    #[test]
    fn a_state_that_marks_nothing_encodes_exactly_as_before() {
        // Every construction site written before the mask existed gets `Default`, and this
        // is the assertion that says those callers did not change behaviour. Without it the
        // mask is a silent rewrite of every historical state in the replay buffer.
        let s = TradeState { pool_age_secs: 120.0, ..Default::default() };
        assert_eq!(s.unmeasured, FeatureMask::NONE);
        assert_eq!(s.to_vec(), s.to_vec_raw());
        assert_eq!(s.coverage(), 1.0);
    }

    #[test]
    fn an_unmeasured_pool_age_is_neutral_and_not_a_newborn_pool() {
        // The finding this exists for. `pool_age_secs: 0.0` normalises to 0.0 — the bottom
        // of the 0..1h band — which reads as a pool that opened this second, the most
        // bullish value the feature can take. `measure` reports it non-zero in 0 of 8,422
        // decisions, so the net has been told that about essentially every pool.
        let mut s = TradeState::default();
        assert_eq!(s.to_vec()[0], 0.0, "the old encoding");
        s.unmeasured.mark_named("pool_age_secs");
        assert_eq!(s.to_vec()[0], NEUTRAL[0]);
        assert!(s.to_vec()[0] > 0.0, "neutral must not be the extreme of the range");
    }

    #[test]
    fn the_neutral_for_an_asymmetric_encoding_is_not_the_midpoint() {
        // The trap in the obvious version of this change. `price_change_pct` is
        // `clamp(-1, 3) / 3`, so a 0% change sits at 0.0 and the midpoint of [0,1] is
        // **+150%**; `buy_sell_ratio` is `/ 5.0`, so a balanced book sits at 0.2. Filling
        // either with 0.5 would replace "I do not know" with "strongly bullish".
        let change = STATE_FEATURES.iter().position(|f| *f == "price_change_pct").unwrap();
        let ratio = STATE_FEATURES.iter().position(|f| *f == "buy_sell_ratio").unwrap();
        assert_eq!(NEUTRAL[change], 0.0);
        assert_eq!(NEUTRAL[ratio], 0.2);

        // And they really are what the measured-neutral input encodes to.
        let flat = TradeState { price_change_pct: 0.0, buy_sell_ratio: 1.0, ..Default::default() };
        assert!((flat.to_vec_raw()[change] - NEUTRAL[change]).abs() < 1e-12);
        assert!((flat.to_vec_raw()[ratio] - NEUTRAL[ratio]).abs() < 1e-12);
    }

    #[test]
    fn every_neutral_is_inside_the_encoded_range() {
        // A neutral outside [0, 1] would be a value the encoder can never produce, so the
        // net would learn to read it as a sentinel — which is a mask, smuggled in without
        // the honesty of being one.
        for (i, v) in NEUTRAL.iter().enumerate() {
            assert!((0.0..=1.0).contains(v), "{} neutral {v}", STATE_FEATURES[i]);
        }
    }

    #[test]
    fn marking_a_feature_lowers_coverage_by_one_feature() {
        let mut s = TradeState::default();
        assert_eq!(s.coverage(), 1.0);
        s.unmeasured.mark_named("lp_burned");
        s.unmeasured.mark_named("mint_renounced");
        assert_eq!(s.unmeasured.count(), 2);
        assert!((s.coverage() - (STATE_DIM - 2) as f64 / STATE_DIM as f64).abs() < 1e-12);
        assert_eq!(s.unmeasured.names(), vec!["lp_burned", "mint_renounced"]);
    }

    #[test]
    fn an_unknown_feature_name_is_ignored_rather_than_panicking() {
        // Reached from a live buy path. A typo must not take the process down, and the
        // count is what makes the mistake visible instead.
        let mut m = FeatureMask::NONE;
        m.mark_named("pool_age_seconds");
        assert_eq!(m.count(), 0);
    }

    #[test]
    fn the_mask_survives_a_round_trip_and_an_old_state_still_loads() {
        let s = TradeState { unmeasured: FeatureMask::NONE.with("volatility"), ..Default::default() };
        let json = serde_json::to_string(&s).unwrap();
        let back: TradeState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.unmeasured, s.unmeasured);

        // A state written before the field existed: every other key present, `unmeasured`
        // absent. The replay buffer inside a checkpoint is full of them, and a deserialise
        // that failed here would make every saved agent unloadable. (Only `unmeasured`
        // carries `#[serde(default)]`; the rest of the struct is still required, which is
        // why this strips one key rather than passing a fragment.)
        let mut doc = serde_json::to_value(TradeState {
            pool_age_secs: 42.0,
            ..Default::default()
        })
        .unwrap();
        doc.as_object_mut().unwrap().remove("unmeasured").expect("the field should be there");
        let old: TradeState = serde_json::from_value(doc).unwrap();
        assert_eq!(old.unmeasured, FeatureMask::NONE);
        assert_eq!(old.pool_age_secs, 42.0);
    }

    #[test]
    fn substitution_touches_only_the_marked_features() {
        let mut s = TradeState { pool_age_secs: 1800.0, volatility: 0.9, ..Default::default() };
        let before = s.to_vec_raw();
        s.unmeasured.mark_named("volatility");
        let after = s.to_vec();
        let vol = STATE_FEATURES.iter().position(|f| *f == "volatility").unwrap();
        for i in 0..STATE_DIM {
            if i == vol {
                assert_eq!(after[i], NEUTRAL[i]);
            } else {
                assert_eq!(after[i], before[i], "{} moved", STATE_FEATURES[i]);
            }
        }
    }

    #[test]
    fn a_fully_unmeasured_state_encodes_to_the_neutral_vector() {
        // The degenerate case, and it should look degenerate: coverage 0, and a vector
        // carrying no information rather than one that happens to read as a fresh, safe,
        // calm pool.
        let mut s = TradeState { pool_age_secs: 5.0, lp_burned: true, ..Default::default() };
        for f in STATE_FEATURES {
            s.unmeasured.mark_named(f);
        }
        assert_eq!(s.coverage(), 0.0);
        assert_eq!(s.to_vec(), NEUTRAL.to_vec());
    }

    #[test]
    fn feature_names_match_vector_order() {
        // The one way STATE_FEATURES can rot is `to_vec` gaining or losing a term
        // without this list following. An exported ONNX model carries these names as
        // its input schema, so a mismatch mislabels every feature downstream.
        assert_eq!(STATE_FEATURES.len(), STATE_DIM);
        assert_eq!(TradeState::default().to_vec().len(), STATE_FEATURES.len());
    }

    #[test]
    fn feature_names_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for name in STATE_FEATURES {
            assert!(seen.insert(name), "duplicate feature name: {name}");
        }
    }

    #[test]
    fn to_vec_is_normalised() {
        // Every feature is documented as living in [0, 1]; the ONNX export states that
        // as its `input_normalisation` metadata, so it had better be true.
        let state = TradeState::default();
        for (name, value) in STATE_FEATURES.iter().zip(state.to_vec()) {
            assert!(
                (0.0..=1.0).contains(&value),
                "{name} = {value} is outside [0, 1]"
            );
        }
    }
}
