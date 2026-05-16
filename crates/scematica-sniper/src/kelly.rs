/// Kelly Criterion position sizing.
///
/// Tracks win rate and average win/loss from recent trade history, then
/// applies a fractional Kelly formula to produce a position-size multiplier.
///
/// Formula: f* = (p * b - q) / b
///   p = win rate, q = 1 - p, b = avg_win / avg_loss
///
/// The raw Kelly fraction is further scaled by `fraction` (default 0.25 =
/// quarter-Kelly) to be conservative.  Result is clamped to 0.25 ..= 3.0.
pub struct KellySizer {
    /// Scaling factor applied to the raw Kelly fraction (e.g. 0.25 for quarter-Kelly)
    fraction: f64,
}

impl KellySizer {
    pub fn new(fraction: f64) -> Self {
        Self {
            fraction: fraction.max(0.01).min(1.0),
        }
    }

    /// Compute a position-size multiplier from recent trade history.
    ///
    /// `history` is a slice of `(profitable, pnl_sol)` tuples.
    /// Returns a multiplier in [0.25, 3.0]:
    ///   - 1.0 means trade at the base quote amount
    ///   - > 1.0 means trade more aggressively
    ///   - < 1.0 means reduce position size
    pub fn compute_multiplier(&self, history: &[(bool, f64)]) -> f64 {
        if history.is_empty() {
            return 1.0;
        }

        let wins: Vec<f64> = history.iter()
            .filter(|(profitable, _)| *profitable)
            .map(|(_, pnl)| pnl.abs())
            .collect();

        let losses: Vec<f64> = history.iter()
            .filter(|(profitable, pnl)| !profitable && pnl.abs() > 1e-9)
            .map(|(_, pnl)| pnl.abs())
            .collect();

        let p = wins.len() as f64 / history.len() as f64;
        let q = 1.0 - p;

        if losses.is_empty() || wins.is_empty() {
            // Not enough data to compute ratio — stay at base
            return 1.0;
        }

        let avg_win = wins.iter().sum::<f64>() / wins.len() as f64;
        let avg_loss = losses.iter().sum::<f64>() / losses.len() as f64;

        if avg_loss < 1e-9 {
            return 1.0;
        }

        let b = avg_win / avg_loss;

        // Kelly formula
        let raw_kelly = (p * b - q) / b;

        // Apply fraction (quarter-Kelly etc.) and convert to multiplier
        let adjusted = raw_kelly * self.fraction;

        // 1.0 = base, scale so 0 Kelly → 0.25x, full Kelly * fraction → up to 3.0x
        let multiplier = 1.0 + adjusted;

        // Clamp to safe range
        multiplier.max(0.25_f64).min(3.0_f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_history() {
        let k = KellySizer::new(0.25);
        assert_eq!(k.compute_multiplier(&[]), 1.0);
    }

    #[test]
    fn test_all_wins() {
        let k = KellySizer::new(0.25);
        let history: Vec<(bool, f64)> = vec![(true, 0.1); 10];
        // All wins, no losses → returns 1.0 (missing loss data)
        assert_eq!(k.compute_multiplier(&history), 1.0);
    }

    #[test]
    fn test_clamped() {
        let k = KellySizer::new(0.25);
        // Extreme negative edge — should still clamp to 0.25
        let history: Vec<(bool, f64)> = vec![(false, 0.5); 20];
        let mult = k.compute_multiplier(&history);
        assert!(mult >= 0.25 && mult <= 3.0, "mult={}", mult);
    }
}
