/// Time-of-day position-size weighting based on UTC trading hours.
///
/// Calibrated from 573 confirmed live trades (2026-05-16 to 2026-05-23):
///   Best hours:  03 (53% WR), 10 (50% WR), 16 (52% WR), 22-23 (39-41% WR)
///   Worst hours: 01 (0% WR), 21 (0% WR), 02 (23% WR), 04 (22% WR)
///   High-volume peak: 14 UTC (27% WR but +0.317 SOL — large winners)
pub struct DayWeighter;

impl DayWeighter {
    /// Return a multiplier for the given UTC hour (0–23).
    ///
    /// - Hours 03, 10, 14-17, 22-23: proven high-quality → 1.3×
    /// - Hours 01, 21: historically 0% win rate → 0.5× (hard reduction)
    /// - Hours 02, 04, 19: weak signal → 0.8×
    /// - All other hours: → 1.0×
    pub fn multiplier_for_hour(utc_hour: u8) -> f64 {
        match utc_hour {
            3 | 10 | 14..=17 | 22 | 23 => 1.3,
            1 | 21                      => 0.5,
            2 | 4 | 19                  => 0.8,
            _                           => 1.0,
        }
    }

    /// Return the multiplier for the current wall-clock UTC hour.
    pub fn current_multiplier() -> f64 {
        let hour = chrono::Utc::now().format("%H").to_string()
            .parse::<u8>()
            .unwrap_or(12);
        Self::multiplier_for_hour(hour)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_peak_hours() {
        for h in 14..=17u8 {
            assert_eq!(DayWeighter::multiplier_for_hour(h), 1.3, "hour={}", h);
        }
        assert_eq!(DayWeighter::multiplier_for_hour(3), 1.3);
        assert_eq!(DayWeighter::multiplier_for_hour(10), 1.3);
        assert_eq!(DayWeighter::multiplier_for_hour(22), 1.3);
        assert_eq!(DayWeighter::multiplier_for_hour(23), 1.3);
    }

    #[test]
    fn test_dead_hours() {
        assert_eq!(DayWeighter::multiplier_for_hour(1), 0.5);
        assert_eq!(DayWeighter::multiplier_for_hour(21), 0.5);
    }

    #[test]
    fn test_weak_hours() {
        assert_eq!(DayWeighter::multiplier_for_hour(2), 0.8);
        assert_eq!(DayWeighter::multiplier_for_hour(4), 0.8);
    }

    #[test]
    fn test_normal() {
        assert_eq!(DayWeighter::multiplier_for_hour(8), 1.0);
        assert_eq!(DayWeighter::multiplier_for_hour(20), 1.0);
    }
}
