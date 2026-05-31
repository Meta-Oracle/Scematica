/// Fibonacci Recovery System - Maximize PnL through pattern-based entry/exit
///
/// Based on live data analysis showing:
/// - 90% losses at -0.499% within 5-45s (dead pools)
/// - 100% wins exit within 0.1-6s at Fibonacci levels
/// - Winning pools: 8-21 SOL, <5s age, >1.618 SOL/s velocity
///
/// This system uses Fibonacci ratios to:
/// 1. Gate entries (only pools matching golden ratio patterns)
/// 2. Set dynamic TP levels (61.8%, 161.8%, 261.8%, 423.6%)
/// 3. Implement fast dead-pool detection (3s timeout)
/// 4. Scale position size by Fibonacci confidence score
use crate::fibonacci_momentum::{FibonacciMomentum, FibonacciSignal, PHI};
use crate::fibonacci_pool_scorer::FibonacciPoolScorer;
use std::time::{SystemTime, UNIX_EPOCH};

/// Recovery-focused configuration derived from live data patterns
#[derive(Debug, Clone)]
pub struct FibonacciRecoveryConfig {
    /// Minimum Fibonacci score to enter (0.0-1.0). Set to 0.75 for high-conviction only
    pub min_entry_score: f64,

    /// Dead-pool timeout in seconds. Data shows 3s is optimal (all wins exit by 6s)
    pub dead_pool_timeout_secs: u64,

    /// Minimum gain % to avoid dead-pool exit. Set to 5% (above AMM spread)
    pub dead_pool_min_gain_pct: f64,

    /// Tiered TP levels based on Fibonacci extensions
    pub tp_levels: Vec<FibonacciTPLevel>,

    /// Enable fast-exit mode (exit at first Fibonacci level hit)
    pub fast_exit_mode: bool,

    /// Position size multiplier based on Fibonacci score
    pub use_fibonacci_sizing: bool,
}

impl Default for FibonacciRecoveryConfig {
    fn default() -> Self {
        Self {
            min_entry_score: 0.50, // Moderate gate — velocity bypass handles exceptional pools below this
            dead_pool_timeout_secs: 5, // 3s was too fast — slow pumpers need >3s to develop
            dead_pool_min_gain_pct: 2.0, // 5% was above AMM spread but cut slow-building winners
            tp_levels: vec![
                FibonacciTPLevel {
                    gain_pct: 61.8,
                    sell_pct: 30.0,
                }, // φ - 1
                FibonacciTPLevel {
                    gain_pct: 161.8,
                    sell_pct: 40.0,
                }, // φ
                FibonacciTPLevel {
                    gain_pct: 261.8,
                    sell_pct: 30.0,
                }, // φ²
            ],
            fast_exit_mode: true, // Exit at first TP hit (matches live data)
            use_fibonacci_sizing: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct FibonacciTPLevel {
    pub gain_pct: f64,
    pub sell_pct: f64,
}

/// Entry decision with Fibonacci analysis
#[derive(Debug, Clone)]
pub struct FibonacciEntryDecision {
    pub should_enter: bool,
    pub fibonacci_score: f64,
    pub position_multiplier: f64,
    pub reason: String,
    pub expected_tp_pct: f64,
}

/// Exit decision from Fibonacci momentum analysis
#[derive(Debug, Clone)]
pub struct FibonacciExitDecision {
    pub should_exit: bool,
    pub exit_reason: String,
    pub expected_pnl_pct: f64,
    pub is_dead_pool: bool,
}

/// Main recovery system coordinator
pub struct FibonacciRecoverySystem {
    config: FibonacciRecoveryConfig,
}

impl FibonacciRecoverySystem {
    pub fn new(config: FibonacciRecoveryConfig) -> Self {
        Self { config }
    }

    /// Evaluate pool for entry using Fibonacci pattern matching
    ///
    /// Returns entry decision with position sizing recommendation
    pub fn evaluate_entry(
        &self,
        pool_size_lamports: u64,
        base_vault_lamports: u64,
        detected_at_secs: u64,
    ) -> FibonacciEntryDecision {
        let size_sol = pool_size_lamports as f64 / 1e9;

        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let age_secs = if detected_at_secs > 0 && detected_at_secs <= now_secs {
            now_secs.saturating_sub(detected_at_secs)
        } else {
            0
        };

        // Calculate velocity (SOL/s)
        let velocity_sol_per_sec = if age_secs > 0 {
            size_sol / age_secs as f64
        } else {
            0.0
        };

        // Calculate buy pressure ratio
        let buy_pressure_ratio = if base_vault_lamports > 0 {
            pool_size_lamports as f64 / base_vault_lamports as f64
        } else {
            0.0
        };

        // Get Fibonacci pattern score
        let fib_score = FibonacciMomentum::score_pool_fibonacci(
            size_sol,
            age_secs,
            velocity_sol_per_sec,
            buy_pressure_ratio,
        );

        // Check if pool meets entry criteria
        let should_enter = fib_score >= self.config.min_entry_score;

        // Calculate position multiplier based on Fibonacci score
        let position_multiplier = if self.config.use_fibonacci_sizing {
            FibonacciPoolScorer::fibonacci_position_multiplier(fib_score)
        } else {
            1.0
        };

        // Determine expected TP based on score
        let expected_tp_pct = if fib_score >= 0.9 {
            261.8 // φ² - exceptional pools
        } else if fib_score >= 0.8 {
            161.8 // φ - strong pools
        } else {
            61.8 // φ - 1 - moderate pools
        };

        let reason = if !should_enter {
            format!(
                "Fibonacci score {:.2} below threshold {:.2}",
                fib_score, self.config.min_entry_score
            )
        } else if fib_score >= 0.9 {
            format!(
                "EXCEPTIONAL: Fibonacci score {:.2} - Perfect golden ratio pattern detected",
                fib_score
            )
        } else if fib_score >= 0.8 {
            format!(
                "STRONG: Fibonacci score {:.2} - Runner characteristics confirmed",
                fib_score
            )
        } else {
            format!(
                "MODERATE: Fibonacci score {:.2} - Acceptable entry pattern",
                fib_score
            )
        };

        FibonacciEntryDecision {
            should_enter,
            fibonacci_score: fib_score,
            position_multiplier,
            reason,
            expected_tp_pct,
        }
    }

    /// Evaluate position for exit using Fibonacci momentum tracking
    ///
    /// Implements:
    /// - Dead pool detection (3s timeout, <5% gain)
    /// - Fibonacci TP levels (61.8%, 161.8%, 261.8%)
    /// - Golden retracement exits (61.8% pullback from peak)
    pub fn evaluate_exit(
        &self,
        momentum: &FibonacciMomentum,
        current_value: u64,
        current_time: u64,
        pool_size_lamports: u64,
    ) -> FibonacciExitDecision {
        // Get Fibonacci signal
        let mut momentum_clone = momentum.clone();
        let signal = momentum_clone.update(current_value, current_time, pool_size_lamports);

        // Check for dead pool (critical for loss prevention)
        let entry_time = momentum.entry_time;
        let age_secs = current_time.saturating_sub(entry_time);

        let current_gain_pct = if momentum.entry_value > 0 {
            ((current_value as f64 - momentum.entry_value as f64) / momentum.entry_value as f64)
                * 100.0
        } else {
            0.0
        };

        let peak_gain_pct = if momentum.entry_value > 0 {
            ((momentum.peak_value as f64 - momentum.entry_value as f64)
                / momentum.entry_value as f64)
                * 100.0
        } else {
            0.0
        };

        // Dead pool detection: exit if no movement after timeout
        if age_secs >= self.config.dead_pool_timeout_secs
            && peak_gain_pct < self.config.dead_pool_min_gain_pct
        {
            return FibonacciExitDecision {
                should_exit: true,
                exit_reason: format!(
                    "DEAD POOL: No pump after {}s (peak {:.1}% < {:.1}%)",
                    age_secs, peak_gain_pct, self.config.dead_pool_min_gain_pct
                ),
                expected_pnl_pct: current_gain_pct,
                is_dead_pool: true,
            };
        }

        // Fibonacci signals: only dead-pool detection triggers an exit here.
        // TP and retracement exits are handled by the main momentum-escalation ladder
        // (trailing stop + pullback exit) so we never cut runners short at 61.8%.
        match signal {
            FibonacciSignal::TakeProfitAtFib {
                gain_pct,
                fib_level,
            } => {
                // Log the level but let the main system decide when to exit
                FibonacciExitDecision {
                    should_exit: false,
                    exit_reason: format!(
                        "FIB TP signal {:.0}% (level {}) — deferring to momentum ladder",
                        gain_pct, fib_level
                    ),
                    expected_pnl_pct: gain_pct,
                    is_dead_pool: false,
                }
            }
            FibonacciSignal::ExitGoldenRetrace {
                peak_gain_pct,
                current_gain_pct,
                retrace_pct,
            } => {
                // Log but defer — main trailing stop handles retracements
                FibonacciExitDecision {
                    should_exit: false,
                    exit_reason: format!(
                        "Golden retrace {:.1}% from {:.1}% peak — deferring to trailing stop",
                        retrace_pct, peak_gain_pct
                    ),
                    expected_pnl_pct: current_gain_pct,
                    is_dead_pool: false,
                }
            }
            FibonacciSignal::ExitVelocityCollapse {
                peak_gain_pct: _,
                current_gain_pct,
                velocity_ratio,
            } => {
                // Velocity collapse — defer to main system
                FibonacciExitDecision {
                    should_exit: false,
                    exit_reason: format!(
                        "Velocity collapse at {:.1}% (ratio {:.2}) — deferring",
                        current_gain_pct, velocity_ratio
                    ),
                    expected_pnl_pct: current_gain_pct,
                    is_dead_pool: false,
                }
            }
            FibonacciSignal::EscalateToNextFib {
                current_gain_pct,
                next_target_pct,
                velocity_ratio,
            } => {
                // Don't exit - escalate TP target
                FibonacciExitDecision {
                    should_exit: false,
                    exit_reason: format!(
                        "ESCALATING: Strong momentum (ratio {:.2}), next target {:.0}%",
                        velocity_ratio, next_target_pct
                    ),
                    expected_pnl_pct: current_gain_pct,
                    is_dead_pool: false,
                }
            }
            FibonacciSignal::RunnerDetected {
                velocity_ratio,
                age_secs,
            } => {
                // Don't exit - runner detected
                FibonacciExitDecision {
                    should_exit: false,
                    exit_reason: format!(
                        "RUNNER DETECTED: Fibonacci velocity {:.2}φ at {}s",
                        velocity_ratio / PHI,
                        age_secs
                    ),
                    expected_pnl_pct: current_gain_pct,
                    is_dead_pool: false,
                }
            }
            FibonacciSignal::Hold {
                gain_pct,
                velocity_ratio,
                next_target_pct,
            } => {
                // Don't exit - hold for target
                FibonacciExitDecision {
                    should_exit: false,
                    exit_reason: format!(
                        "HOLDING: {:.1}% gain, target {:.0}% (velocity {:.2})",
                        gain_pct, next_target_pct, velocity_ratio
                    ),
                    expected_pnl_pct: gain_pct,
                    is_dead_pool: false,
                }
            }
        }
    }

    /// Calculate optimal position size using Fibonacci progression
    ///
    /// After each win, scale next position by Fibonacci ratio to compound gains
    pub fn calculate_position_size(
        &self,
        base_size: f64,
        consecutive_wins: u32,
        fibonacci_score: f64,
    ) -> f64 {
        let mut size = base_size;

        // Apply Fibonacci progression for consecutive wins
        if consecutive_wins > 0 {
            let win_multiplier =
                FibonacciMomentum::calculate_position_multiplier(consecutive_wins, 1.0);
            size *= win_multiplier;
        }

        // Apply Fibonacci score multiplier
        if self.config.use_fibonacci_sizing {
            let score_multiplier =
                FibonacciPoolScorer::fibonacci_position_multiplier(fibonacci_score);
            size *= score_multiplier;
        }

        size
    }

    /// Get recommended TP level based on current gain and Fibonacci analysis
    pub fn get_next_tp_level(&self, current_gain_pct: f64) -> Option<&FibonacciTPLevel> {
        self.config
            .tp_levels
            .iter()
            .find(|level| current_gain_pct < level.gain_pct)
    }

    /// Check if pool is a Fibonacci runner (fast-lane entry)
    pub fn is_fibonacci_runner(
        &self,
        pool_size_lamports: u64,
        age_secs: u64,
        velocity_sol_per_sec: f64,
        buy_pressure_ratio: f64,
    ) -> bool {
        FibonacciPoolScorer::is_fibonacci_runner(
            pool_size_lamports,
            age_secs,
            velocity_sol_per_sec,
            buy_pressure_ratio,
        )
    }
}

/// Recovery statistics tracker
#[derive(Debug, Clone, Default)]
pub struct FibonacciRecoveryStats {
    pub total_entries: u32,
    pub fibonacci_entries: u32,
    pub dead_pool_exits: u32,
    pub fibonacci_tp_exits: u32,
    pub golden_retrace_exits: u32,
    pub total_pnl_lamports: i64,
    pub avg_fibonacci_score: f64,
    pub avg_hold_time_secs: f64,
}

impl FibonacciRecoveryStats {
    pub fn record_entry(&mut self, fibonacci_score: f64) {
        self.total_entries += 1;
        if fibonacci_score >= 0.75 {
            self.fibonacci_entries += 1;
        }

        // Update running average
        let n = self.total_entries as f64;
        self.avg_fibonacci_score = (self.avg_fibonacci_score * (n - 1.0) + fibonacci_score) / n;
    }

    pub fn record_exit(
        &mut self,
        decision: &FibonacciExitDecision,
        hold_time_secs: u64,
        pnl_lamports: i64,
    ) {
        if decision.is_dead_pool {
            self.dead_pool_exits += 1;
        } else if decision.exit_reason.contains("FIBONACCI TP") {
            self.fibonacci_tp_exits += 1;
        } else if decision.exit_reason.contains("GOLDEN RETRACEMENT") {
            self.golden_retrace_exits += 1;
        }

        self.total_pnl_lamports += pnl_lamports;

        // Update running average hold time
        let n = (self.dead_pool_exits + self.fibonacci_tp_exits + self.golden_retrace_exits) as f64;
        if n > 0.0 {
            self.avg_hold_time_secs =
                (self.avg_hold_time_secs * (n - 1.0) + hold_time_secs as f64) / n;
        }
    }

    pub fn win_rate(&self) -> f64 {
        let total_exits =
            self.dead_pool_exits + self.fibonacci_tp_exits + self.golden_retrace_exits;
        if total_exits == 0 {
            return 0.0;
        }
        (self.fibonacci_tp_exits + self.golden_retrace_exits) as f64 / total_exits as f64
    }

    pub fn avg_pnl_sol(&self) -> f64 {
        let total_exits =
            self.dead_pool_exits + self.fibonacci_tp_exits + self.golden_retrace_exits;
        if total_exits == 0 {
            return 0.0;
        }
        self.total_pnl_lamports as f64 / 1e9 / total_exits as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dead_pool_detection() {
        let config = FibonacciRecoveryConfig::default();
        let system = FibonacciRecoverySystem::new(config);

        let entry_value = 10_000_000; // 0.01 SOL
        let entry_time = 1000;
        let momentum = FibonacciMomentum::new(entry_value, entry_time);

        // Simulate dead pool: 4 seconds, no gain
        let current_time = entry_time + 4;
        let current_value = entry_value; // No movement
        let pool_size = 15_000_000_000; // 15 SOL

        let decision = system.evaluate_exit(&momentum, current_value, current_time, pool_size);

        assert!(decision.should_exit, "Should exit dead pool");
        assert!(decision.is_dead_pool, "Should be flagged as dead pool");
    }

    #[test]
    fn test_fibonacci_runner_detection() {
        let config = FibonacciRecoveryConfig::default();
        let system = FibonacciRecoverySystem::new(config);

        // Perfect Fibonacci runner: 13 SOL, 3s old, 4.0 SOL/s, 2.0 pressure
        assert!(system.is_fibonacci_runner(13_000_000_000, 3, 4.0, 2.0));

        // Not a runner: too old
        assert!(!system.is_fibonacci_runner(13_000_000_000, 10, 4.0, 2.0));
    }

    #[test]
    fn test_entry_gating() {
        let config = FibonacciRecoveryConfig::default();
        let system = FibonacciRecoverySystem::new(config);

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // High-quality pool: 13 SOL, 3s old
        let decision = system.evaluate_entry(13_000_000_000, 500_000_000, now - 3);

        assert!(
            decision.fibonacci_score >= 0.35,
            "Should have high Fibonacci score"
        );
        assert!(decision.should_enter, "Should enter high-quality pool");
    }

    #[test]
    fn test_position_sizing() {
        let config = FibonacciRecoveryConfig::default();
        let system = FibonacciRecoverySystem::new(config);

        let base_size = 0.01;

        // No wins, high score
        let size1 = system.calculate_position_size(base_size, 0, 0.9);
        assert!(size1 > base_size, "High score should increase size");

        // 3 consecutive wins (Fibonacci 2.0x)
        let size2 = system.calculate_position_size(base_size, 3, 0.9);
        assert!(size2 > size1, "Consecutive wins should compound size");
    }
}
