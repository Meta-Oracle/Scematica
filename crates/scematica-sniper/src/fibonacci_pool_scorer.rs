/// Enhanced pool scorer integrating Fibonacci momentum analysis
///
/// This module combines the existing Bayesian pool scorer with Fibonacci
/// pattern recognition to maximize runner detection and PnL recovery.
use crate::cache::CachedPool;
use crate::fibonacci_momentum::FibonacciMomentum;
use crate::pool_scorer::PoolScorer;

/// Enhanced pool scoring with Fibonacci analysis
pub struct FibonacciPoolScorer;

impl FibonacciPoolScorer {
    /// Score a pool using combined Bayesian + Fibonacci analysis
    ///
    /// Returns a score 0.0-100.0 where:
    /// - Base score from Bayesian model (0-100)
    /// - Fibonacci bonus (+0 to +15) for pools matching golden ratio patterns
    /// - Runner detection bonus (+0 to +10) for exceptional velocity patterns
    ///
    /// # Integration Strategy
    /// 1. Run existing Bayesian scorer (proven calibration)
    /// 2. Apply Fibonacci pattern bonus (additive, not multiplicative)
    /// 3. Cap final score at 100.0
    ///
    /// This preserves the existing scoring while rewarding pools that exhibit
    /// Fibonacci momentum characteristics proven to correlate with runners.
    pub fn score_with_fibonacci(
        pool: &CachedPool,
        pool_size_lamports: u64,
        base_vault_lamports: u64,
        detected_at_secs: u64,
        social_count: u8,
    ) -> f64 {
        // Get base Bayesian score
        let base_score = if social_count > 0 {
            PoolScorer::score_with_socials(
                pool,
                pool_size_lamports,
                base_vault_lamports,
                detected_at_secs,
                social_count,
            )
        } else {
            PoolScorer::score(
                pool,
                pool_size_lamports,
                base_vault_lamports,
                detected_at_secs,
            )
        };

        // Calculate Fibonacci metrics
        let size_sol = pool_size_lamports as f64 / 1e9;

        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let effective_open_time = if pool.open_time > 0 {
            pool.open_time
        } else if detected_at_secs > 0 && detected_at_secs >= now_secs.saturating_sub(60) {
            detected_at_secs
        } else {
            0
        };

        let age_secs = if effective_open_time > 0 && effective_open_time <= now_secs {
            now_secs.saturating_sub(effective_open_time)
        } else {
            0
        };

        // Calculate velocity — same fallback logic as pool_scorer.rs:
        // when open_time=0 (all pump.fun migrations), use a conservative 30s age
        // estimate rather than returning 0.0 which kills Fibonacci signal entirely.
        let velocity_sol_per_sec =
            if pool_size_lamports > 0 && pool.open_time > 0 && pool.open_time <= now_secs {
                let age_s = now_secs.saturating_sub(pool.open_time).max(1) as f64;
                size_sol / age_s
            } else if pool_size_lamports > 0
                && detected_at_secs > 0
                && detected_at_secs >= now_secs.saturating_sub(60)
            {
                size_sol / 30.0 // conservative 30s age estimate for freshly-detected open_time=0 pools
            } else {
                0.0
            };

        // Calculate buy pressure ratio
        let buy_pressure_ratio = if base_vault_lamports > 0 {
            pool_size_lamports as f64 / base_vault_lamports as f64
        } else {
            0.0
        };

        // Get Fibonacci pattern score (0.0-1.0)
        let fib_score = FibonacciMomentum::score_pool_fibonacci(
            size_sol,
            age_secs,
            velocity_sol_per_sec,
            buy_pressure_ratio,
        );

        // Convert Fibonacci score to bonus points
        // High Fibonacci score (0.8-1.0) = +10 to +15 points
        // Medium (0.5-0.8) = +5 to +10 points
        // Low (0.0-0.5) = +0 to +5 points
        let fib_bonus = if fib_score >= 0.8 {
            10.0 + (fib_score - 0.8) * 25.0 // +10 to +15
        } else if fib_score >= 0.5 {
            5.0 + (fib_score - 0.5) * 16.67 // +5 to +10
        } else {
            fib_score * 10.0 // +0 to +5
        };

        // Runner detection bonus: exceptional velocity matching Fibonacci growth
        let runner_bonus = if velocity_sol_per_sec >= 3.236 && age_secs <= 8 {
            // φ² (2.618) SOL/s in first 8 seconds = strong runner signal
            10.0
        } else if velocity_sol_per_sec >= 1.618 && age_secs <= 13 {
            // φ (1.618) SOL/s in first 13 seconds = moderate runner
            5.0
        } else {
            0.0
        };

        // Combine scores
        let final_score = base_score + fib_bonus + runner_bonus;

        // Cap at 100.0
        final_score.min(100.0)
    }

    /// Quick check if pool exhibits strong Fibonacci runner characteristics
    ///
    /// Returns true if the pool should be prioritized for immediate entry
    /// based on Fibonacci momentum patterns, regardless of base score.
    ///
    /// Use this as a "fast lane" gate: pools that pass this check bypass
    /// normal scoring delays and enter immediately.
    pub fn is_fibonacci_runner(
        pool_size_lamports: u64,
        age_secs: u64,
        velocity_sol_per_sec: f64,
        buy_pressure_ratio: f64,
    ) -> bool {
        let size_sol = pool_size_lamports as f64 / 1e9;

        // Fibonacci runner criteria (all must be true):
        // 1. Pool size in golden range: 8-21 SOL (F(6) to F(8))
        // 2. Ultra-fresh: ≤ 5 seconds old (F(5))
        // 3. Velocity ≥ φ² (2.618 SOL/s) - exceptional momentum
        // 4. Buy pressure ≥ φ (1.618) - strong demand

        size_sol >= 8.0
            && size_sol <= 21.0
            && age_secs <= 5
            && velocity_sol_per_sec >= 2.618
            && buy_pressure_ratio >= 1.618
    }

    /// Calculate recommended position size multiplier based on Fibonacci score
    ///
    /// Higher Fibonacci scores warrant larger positions due to increased
    /// runner probability. Returns multiplier 0.5x to 2.0x.
    pub fn fibonacci_position_multiplier(fib_score: f64) -> f64 {
        if fib_score >= 0.9 {
            2.0 // Exceptional Fibonacci pattern - double position
        } else if fib_score >= 0.75 {
            1.618 // Strong pattern - golden ratio multiplier
        } else if fib_score >= 0.5 {
            1.0 // Moderate pattern - baseline
        } else if fib_score >= 0.25 {
            0.75 // Weak pattern - reduce exposure
        } else {
            0.5 // Poor pattern - minimum position
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;

    fn make_pool(open_time: u64) -> CachedPool {
        CachedPool {
            id: Pubkey::new_unique(),
            base_mint: Pubkey::new_unique(),
            quote_mint: Pubkey::new_unique(),
            base_vault: Pubkey::new_unique(),
            quote_vault: Pubkey::new_unique(),
            market_id: Pubkey::new_unique(),
            open_time,
            base_decimals: 9,
            quote_decimals: 9,
            pumpfun_score: 0.0,
        }
    }

    #[test]
    fn test_fibonacci_runner_detection() {
        // Perfect Fibonacci runner: 13 SOL, 3s old, 3.0 SOL/s, 2.0 pressure
        assert!(FibonacciPoolScorer::is_fibonacci_runner(
            13_000_000_000,
            3,
            3.0,
            2.0
        ));

        // Not a runner: too old
        assert!(!FibonacciPoolScorer::is_fibonacci_runner(
            13_000_000_000,
            10,
            3.0,
            2.0
        ));

        // Not a runner: weak velocity
        assert!(!FibonacciPoolScorer::is_fibonacci_runner(
            13_000_000_000,
            3,
            1.0,
            2.0
        ));
    }

    #[test]
    fn test_fibonacci_bonus_applied() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Perfect Fibonacci pool
        let pool = make_pool(now - 3);
        let score = FibonacciPoolScorer::score_with_fibonacci(
            &pool,
            13_000_000_000, // 13 SOL
            500_000_000,    // Strong buy pressure
            0,
            2, // Some social presence
        );

        // Should score very high due to Fibonacci bonus
        assert!(
            score >= 90.0,
            "Perfect Fibonacci pool should score ≥90: {}",
            score
        );
    }

    #[test]
    fn test_position_multiplier_scaling() {
        assert_eq!(
            FibonacciPoolScorer::fibonacci_position_multiplier(0.95),
            2.0
        );
        assert_eq!(
            FibonacciPoolScorer::fibonacci_position_multiplier(0.80),
            1.618
        );
        assert_eq!(
            FibonacciPoolScorer::fibonacci_position_multiplier(0.60),
            1.0
        );
        assert_eq!(
            FibonacciPoolScorer::fibonacci_position_multiplier(0.30),
            0.75
        );
        assert_eq!(
            FibonacciPoolScorer::fibonacci_position_multiplier(0.10),
            0.5
        );
    }
}
