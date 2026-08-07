use std::sync::atomic::{AtomicU64, Ordering};

/// All-time-high wallet balance tracker.
///
/// Tracks the highest balance seen during the session.  Used to implement an
/// ATH drawdown circuit breaker: if the wallet drops more than
/// `ath_drawdown_pct` below the ATH, buying is paused.
pub struct AthTracker {
    ath_lamports: AtomicU64,
}

impl AthTracker {
    /// Create a new tracker with no ATH set (starts at 0).
    pub fn new() -> Self {
        Self {
            ath_lamports: AtomicU64::new(0),
        }
    }

    /// Update the ATH if `balance` is higher than the current ATH.
    pub fn update(&self, balance: u64) {
        // Spin until we either set a higher value or discover another thread set one first.
        loop {
            let current = self.ath_lamports.load(Ordering::Relaxed);
            if balance <= current {
                break;
            }
            if self.ath_lamports
                .compare_exchange(current, balance, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break;
            }
        }
    }

    /// Return the current all-time-high in lamports.
    pub fn ath(&self) -> u64 {
        self.ath_lamports.load(Ordering::Relaxed)
    }

    /// Calculate how far `current` is below the ATH as a percentage.
    ///
    /// Returns 0.0 if ATH is 0 (not yet set) or if current >= ATH.
    pub fn drawdown_pct(&self, current: u64) -> f64 {
        let ath = self.ath_lamports.load(Ordering::Relaxed);
        if ath == 0 || current >= ath {
            return 0.0;
        }
        (ath - current) as f64 / ath as f64 * 100.0
    }
}

impl Default for AthTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ath_update() {
        let t = AthTracker::new();
        t.update(100);
        t.update(50);
        t.update(200);
        assert_eq!(t.ath(), 200);
    }

    #[test]
    fn test_drawdown_pct() {
        let t = AthTracker::new();
        t.update(1_000_000_000); // 1 SOL ATH
        let dd = t.drawdown_pct(900_000_000); // 900ms lamports
        assert!((dd - 10.0).abs() < 0.01, "dd={}", dd);
    }

    #[test]
    fn test_no_drawdown_above_ath() {
        let t = AthTracker::new();
        t.update(500);
        assert_eq!(t.drawdown_pct(500), 0.0);
        assert_eq!(t.drawdown_pct(600), 0.0);
    }

    #[test]
    fn test_zero_ath() {
        let t = AthTracker::new();
        assert_eq!(t.drawdown_pct(0), 0.0);
    }
}
