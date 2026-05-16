use std::collections::VecDeque;
use std::sync::Mutex;

/// 5-minute sliding window loss circuit breaker.
///
/// Records PnL events with timestamps and determines whether cumulative losses
/// inside the window exceed the configured limit.  Thread-safe via an internal
/// `Mutex`.
pub struct GriefBreaker {
    /// Size of the sliding window in seconds
    window_secs: u64,
    /// Loss limit in lamports (negative PnL is a loss; i64 for signed arithmetic)
    limit_lamports: i64,
    /// Ring buffer of (unix_timestamp_secs, pnl_lamports) events
    events: Mutex<VecDeque<(u64, i64)>>,
}

impl GriefBreaker {
    /// Create a new breaker.
    ///
    /// - `window_secs`: sliding window size (e.g. 300 for 5 minutes)
    /// - `limit_sol`:   maximum tolerable loss in SOL (0.0 = disabled)
    pub fn new(window_secs: u64, limit_sol: f64) -> Self {
        let limit_lamports = (limit_sol * 1_000_000_000.0) as i64;
        Self {
            window_secs,
            limit_lamports,
            events: Mutex::new(VecDeque::new()),
        }
    }

    /// Record a completed trade's PnL (can be positive or negative).
    pub fn record_pnl(&self, pnl_lamports: i64) {
        let now = Self::now_secs();
        let mut events = self.events.lock().unwrap();
        events.push_back((now, pnl_lamports));
        // Trim events outside the window while we have the lock
        Self::trim(&mut events, now, self.window_secs);
    }

    /// Returns `true` if the cumulative loss inside the window exceeds the limit.
    /// Always returns `false` when `limit_lamports` is 0 (disabled).
    pub fn is_tripped(&self) -> bool {
        if self.limit_lamports == 0 {
            return false;
        }
        let now = Self::now_secs();
        let mut events = self.events.lock().unwrap();
        Self::trim(&mut events, now, self.window_secs);
        let window_loss = Self::window_loss_from_events(&events);
        window_loss >= self.limit_lamports
    }

    /// Total absolute loss (positive number) seen inside the window, in SOL.
    pub fn window_loss_sol(&self) -> f64 {
        let now = Self::now_secs();
        let mut events = self.events.lock().unwrap();
        Self::trim(&mut events, now, self.window_secs);
        let loss_lamports = Self::window_loss_from_events(&events);
        loss_lamports as f64 / 1_000_000_000.0
    }

    // ── private helpers ───────────────────────────────────────────────────────

    fn now_secs() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }

    /// Drop events older than `window_secs`.
    fn trim(events: &mut VecDeque<(u64, i64)>, now: u64, window_secs: u64) {
        let cutoff = now.saturating_sub(window_secs);
        while let Some(&(ts, _)) = events.front() {
            if ts < cutoff {
                events.pop_front();
            } else {
                break;
            }
        }
    }

    /// Sum up *losses* (negative PnL) within the deque and return as a positive lamport amount.
    fn window_loss_from_events(events: &VecDeque<(u64, i64)>) -> i64 {
        events.iter()
            .filter(|(_, pnl)| *pnl < 0)
            .map(|(_, pnl)| pnl.unsigned_abs() as i64)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disabled() {
        let gb = GriefBreaker::new(300, 0.0);
        gb.record_pnl(-1_000_000_000);
        assert!(!gb.is_tripped());
    }

    #[test]
    fn test_trip() {
        let gb = GriefBreaker::new(300, 0.05);
        gb.record_pnl(-50_000_000); // -0.05 SOL loss
        assert!(gb.is_tripped());
    }

    #[test]
    fn test_no_trip_with_wins() {
        let gb = GriefBreaker::new(300, 0.1);
        gb.record_pnl(-30_000_000); // -0.03 SOL
        gb.record_pnl(50_000_000);  // +0.05 SOL (ignored for loss calc)
        assert!(!gb.is_tripped());
    }
}
