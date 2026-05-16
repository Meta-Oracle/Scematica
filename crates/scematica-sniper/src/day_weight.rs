/// Time-of-day position-size weighting based on UTC trading hours.
///
/// Meme-coin activity on Solana peaks in US morning/afternoon (14–17 UTC).
/// Overnight UTC hours (0–5) are quieter — reduce position size to limit
/// exposure to thin liquidity and manipulated pumps.
pub struct DayWeighter;

impl DayWeighter {
    /// Return a multiplier for the given UTC hour (0–23).
    ///
    /// - Hours 14–17 UTC (10am–1pm EST): peak meme-coin hours → 1.3x
    /// - Hours 0–5 UTC (low overnight activity): → 0.7x
    /// - All other hours: → 1.0x
    pub fn multiplier_for_hour(utc_hour: u8) -> f64 {
        match utc_hour {
            14..=17 => 1.3,
            0..=5   => 0.7,
            _        => 1.0,
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
    }

    #[test]
    fn test_overnight() {
        for h in 0..=5u8 {
            assert_eq!(DayWeighter::multiplier_for_hour(h), 0.7, "hour={}", h);
        }
    }

    #[test]
    fn test_normal() {
        assert_eq!(DayWeighter::multiplier_for_hour(10), 1.0);
        assert_eq!(DayWeighter::multiplier_for_hour(20), 1.0);
    }
}
