/// Fibonacci-based momentum analysis for maximizing runner detection and PnL recovery.
///
/// # Core Principle
/// The Fibonacci sequence (1, 1, 2, 3, 5, 8, 13, 21, 34, 55, 89...) appears naturally
/// in market momentum patterns. This module uses Fibonacci ratios to:
///
/// 1. **Detect true runners early** - Pools that maintain Fibonacci-ratio velocity
///    growth are statistically more likely to be sustained pumps vs flash rugs
/// 2. **Optimize exit timing** - Exit at Fibonacci retracement levels (38.2%, 50%, 61.8%)
///    to maximize realized PnL before reversals
/// 3. **Scale position size** - Use Fibonacci progression to compound wins into larger
///    positions on confirmed runners
///
/// # Mathematical Foundation
///
/// ## Golden Ratio (φ = 1.618)
/// The ratio between consecutive Fibonacci numbers converges to φ. Markets naturally
/// retrace to φ-derived levels:
/// - 0.236 (1 - 0.764) = shallow pullback
/// - 0.382 (φ - 1) = moderate retracement  
/// - 0.500 = psychological midpoint
/// - 0.618 (1/φ) = **golden retracement** - strongest support/resistance
/// - 0.786 (√0.618) = deep retracement before reversal
///
/// ## Velocity Fibonacci Sequence
/// Track SOL inflow velocity across Fibonacci time windows:
/// - F(1) = 1s window
/// - F(2) = 1s window  
/// - F(3) = 2s window
/// - F(5) = 5s window
/// - F(8) = 8s window
/// - F(13) = 13s window
/// - F(21) = 21s window
///
/// A true runner maintains velocity ratios close to φ across these windows.
/// Flash pumps show velocity collapse (ratio << φ) after the initial spike.
use std::collections::VecDeque;

/// Fibonacci constants derived from the golden ratio
pub const PHI: f64 = 1.618033988749895; // Golden ratio
pub const PHI_INV: f64 = 0.618033988749895; // 1/φ - primary retracement
pub const PHI_SQ: f64 = 2.618033988749895; // φ² - extension target

/// Fibonacci retracement levels for exit optimization
pub const FIB_RETRACE_23_6: f64 = 0.236;
pub const FIB_RETRACE_38_2: f64 = 0.382;
pub const FIB_RETRACE_50_0: f64 = 0.500;
pub const FIB_RETRACE_61_8: f64 = 0.618; // Golden retracement
pub const FIB_RETRACE_78_6: f64 = 0.786;

/// Fibonacci extension levels for TP targets
pub const FIB_EXT_161_8: f64 = 1.618; // φ
pub const FIB_EXT_261_8: f64 = 2.618; // φ²
pub const FIB_EXT_423_6: f64 = 4.236; // φ³

/// Fibonacci time windows (seconds) for velocity analysis
pub const FIB_WINDOWS: [u64; 7] = [1, 1, 2, 3, 5, 8, 13];

/// Momentum state for a single position
#[derive(Debug, Clone)]
pub struct FibonacciMomentum {
    /// Entry price in lamports
    pub entry_value: u64,
    /// Peak value seen (for retracement calculation)
    pub peak_value: u64,
    /// Velocity measurements across Fibonacci windows
    velocity_windows: VecDeque<VelocityWindow>,
    /// Current Fibonacci level (for progressive TP)
    current_fib_level: usize,
    /// Timestamp of entry (unix seconds)
    pub entry_time: u64,
    /// Last check timestamp
    last_check_time: u64,
}

#[derive(Debug, Clone)]
struct VelocityWindow {
    /// Window size in seconds (Fibonacci number)
    window_secs: u64,
    /// Average SOL/s velocity in this window
    velocity: f64,
    /// Timestamp when this window was measured
    measured_at: u64,
}

impl FibonacciMomentum {
    /// Create new momentum tracker for a position
    pub fn new(entry_value: u64, entry_time: u64) -> Self {
        Self {
            entry_value,
            peak_value: entry_value,
            velocity_windows: VecDeque::with_capacity(FIB_WINDOWS.len()),
            current_fib_level: 0,
            entry_time,
            last_check_time: entry_time,
        }
    }

    /// Update momentum state with new price check
    ///
    /// Returns `FibonacciSignal` indicating recommended action
    pub fn update(
        &mut self,
        current_value: u64,
        current_time: u64,
        pool_size_lamports: u64,
    ) -> FibonacciSignal {
        // Update peak
        if current_value > self.peak_value {
            self.peak_value = current_value;
        }

        let age_secs = current_time.saturating_sub(self.entry_time);
        let _time_since_last = current_time.saturating_sub(self.last_check_time);
        self.last_check_time = current_time;

        // Calculate current gain %
        let gain_pct = if self.entry_value > 0 {
            ((current_value as f64 - self.entry_value as f64) / self.entry_value as f64) * 100.0
        } else {
            0.0
        };

        // Calculate retracement from peak
        let peak_gain_pct = if self.entry_value > 0 {
            ((self.peak_value as f64 - self.entry_value as f64) / self.entry_value as f64) * 100.0
        } else {
            0.0
        };

        let retrace_pct = if self.peak_value > self.entry_value {
            (self.peak_value as f64 - current_value as f64)
                / (self.peak_value as f64 - self.entry_value as f64)
        } else {
            0.0
        };

        // Update velocity windows for each Fibonacci time period
        self.update_velocity_windows(current_value, current_time, pool_size_lamports);

        // Analyze velocity ratios across Fibonacci windows
        let velocity_ratio = self.calculate_velocity_ratio();

        // Early runner detection: strong short-window velocity in the first
        // Fibonacci windows should be classified before TP-level checks.
        if age_secs <= 13 && velocity_ratio >= PHI * 1.2 {
            return FibonacciSignal::RunnerDetected {
                velocity_ratio,
                age_secs,
            };
        }

        // Check for Fibonacci retracement exit signals
        if peak_gain_pct >= 50.0 {
            // Only check retracements after meaningful gains
            if retrace_pct >= FIB_RETRACE_61_8 {
                // Golden retracement hit - strong exit signal
                return FibonacciSignal::ExitGoldenRetrace {
                    peak_gain_pct,
                    current_gain_pct: gain_pct,
                    retrace_pct: retrace_pct * 100.0,
                };
            } else if retrace_pct >= FIB_RETRACE_50_0 && velocity_ratio < 0.8 {
                // 50% retrace + velocity collapse = exit
                return FibonacciSignal::ExitVelocityCollapse {
                    peak_gain_pct,
                    current_gain_pct: gain_pct,
                    velocity_ratio,
                };
            }
        }

        // Check for Fibonacci extension TP targets
        let next_fib_target = self.get_next_fibonacci_target();
        if gain_pct >= next_fib_target {
            // Check if velocity supports continuation
            if velocity_ratio >= PHI * 0.85 {
                // Velocity still strong - escalate to next Fibonacci level
                self.current_fib_level += 1;
                return FibonacciSignal::EscalateToNextFib {
                    current_gain_pct: gain_pct,
                    next_target_pct: self.get_next_fibonacci_target(),
                    velocity_ratio,
                };
            } else {
                // Velocity weakening at Fibonacci level - take profit
                return FibonacciSignal::TakeProfitAtFib {
                    gain_pct,
                    fib_level: self.current_fib_level,
                };
            }
        }

        // Default: hold and continue monitoring
        FibonacciSignal::Hold {
            gain_pct,
            velocity_ratio,
            next_target_pct: next_fib_target,
        }
    }

    /// Update velocity measurements across all Fibonacci windows
    fn update_velocity_windows(
        &mut self,
        _current_value: u64,
        current_time: u64,
        pool_size_lamports: u64,
    ) {
        let age_secs = current_time.saturating_sub(self.entry_time);

        for &window_secs in &FIB_WINDOWS {
            if age_secs >= window_secs {
                // Calculate velocity for this window
                let velocity = if window_secs > 0 && pool_size_lamports > 0 {
                    (pool_size_lamports as f64 / 1e9) / window_secs as f64
                } else {
                    0.0
                };

                // Update or add window measurement
                if let Some(existing) = self
                    .velocity_windows
                    .iter_mut()
                    .find(|w| w.window_secs == window_secs)
                {
                    existing.velocity = velocity;
                    existing.measured_at = current_time;
                } else {
                    self.velocity_windows.push_back(VelocityWindow {
                        window_secs,
                        velocity,
                        measured_at: current_time,
                    });
                }
            }
        }

        // Keep only recent measurements
        self.velocity_windows
            .retain(|w| current_time.saturating_sub(w.measured_at) < 30);
    }

    /// Calculate velocity ratio across Fibonacci windows
    ///
    /// Returns ratio of short-window velocity / long-window velocity.
    /// - Ratio ≈ φ (1.618) = perfect Fibonacci momentum (runner)
    /// - Ratio > φ = accelerating (strong runner)
    /// - Ratio < 1.0 = decelerating (momentum dying)
    fn calculate_velocity_ratio(&self) -> f64 {
        if self.velocity_windows.len() < 2 {
            return 1.0;
        }

        // Compare the shortest available Fibonacci window to the longest.
        let short_window = self.velocity_windows.front().unwrap();
        let long_window = self.velocity_windows.back().unwrap();

        if long_window.velocity > 0.0 {
            short_window.velocity / long_window.velocity
        } else {
            1.0
        }
    }

    /// Get next Fibonacci extension target as gain %
    fn get_next_fibonacci_target(&self) -> f64 {
        match self.current_fib_level {
            0 => 61.8,                                           // First target: 61.8% (φ - 1) * 100
            1 => 161.8,                                          // φ * 100
            2 => 261.8,                                          // φ² * 100
            3 => 423.6,                                          // φ³ * 100
            4 => 685.4,                                          // φ⁴ * 100
            5 => 1109.0,                                         // φ⁵ * 100
            _ => 1618.0 * (self.current_fib_level as f64 - 4.0), // Continue Fibonacci progression
        }
    }

    /// Calculate optimal position size multiplier using Fibonacci progression
    ///
    /// After each winning trade, scale next position by Fibonacci ratio.
    /// This compounds wins geometrically while maintaining risk control.
    pub fn calculate_position_multiplier(consecutive_wins: u32, base_multiplier: f64) -> f64 {
        if consecutive_wins == 0 {
            return base_multiplier;
        }

        // Fibonacci sequence for position sizing: 1, 1, 2, 3, 5, 8, 13...
        // Cap at F(8) = 21 to prevent over-leverage
        let fib_mult = match consecutive_wins {
            1 => 1.0,
            2 => 1.0,
            3 => 2.0,
            4 => 3.0,
            5 => 5.0,
            6 => 8.0,
            7 => 13.0,
            _ => 21.0, // Cap at F(9)
        };

        base_multiplier * fib_mult
    }

    /// Analyze pool for Fibonacci runner characteristics at detection time
    ///
    /// Returns score 0.0-1.0 indicating runner probability based on:
    /// - Pool size in Fibonacci sweet spot (8-21 SOL)
    /// - Age in Fibonacci window (1-8 seconds)
    /// - Velocity matching Fibonacci growth pattern
    ///
    /// `velocity_sol_per_sec` is `None` when velocity could not be measured — which is
    /// the normal case at detection time, because a pool observed once has no growth
    /// history yet. It previously arrived as a literal `0.0`, which fell into the
    /// "weak" bucket and docked **every** pool 0.05 of its score. With age and buy
    /// pressure also near-constant at detection, that left the outcome decided by pool
    /// size alone, and parked the two possible results at 0.395 and 0.4999… — directly
    /// on top of a 0.50 entry threshold. 519 of 543 gate rejections in one month's
    /// decision log were those two values.
    ///
    /// Missing data is not bad data, so an unmeasurable velocity now scores neutral
    /// rather than weak. The gate keeps discriminating on the signals that *do* vary
    /// (size, and buy pressure when it is meaningful); it just stops charging a
    /// uniform penalty for a number nobody could have supplied.
    pub fn score_pool_fibonacci(
        pool_size_sol: f64,
        age_secs: u64,
        velocity_sol_per_sec: Option<f64>,
        buy_pressure_ratio: f64,
    ) -> f64 {
        let mut score: f64 = 0.0;

        // Pool size: Fibonacci sweet spot 8-21 SOL (F(6) to F(8))
        let size_score = if pool_size_sol >= 8.0 && pool_size_sol <= 21.0 {
            1.0 // Perfect Fibonacci range
        } else if pool_size_sol >= 5.0 && pool_size_sol <= 34.0 {
            0.7 // Adjacent Fibonacci numbers (F(5) to F(9))
        } else if pool_size_sol >= 3.0 && pool_size_sol <= 55.0 {
            0.4 // Wider Fibonacci range
        } else {
            0.1 // Outside Fibonacci sweet spots
        };
        score += size_score * 0.35;

        // Age: Ultra-fresh in first Fibonacci window (1-8 seconds)
        let age_score = if age_secs <= 3 {
            1.0 // F(4) = 3s - peak freshness
        } else if age_secs <= 5 {
            0.9 // F(5) = 5s
        } else if age_secs <= 8 {
            0.7 // F(6) = 8s
        } else if age_secs <= 13 {
            0.4 // F(7) = 13s
        } else {
            0.1 // Too old
        };
        score += age_score * 0.30;

        // Velocity: Should match Fibonacci growth (φ ≈ 1.618 SOL/s minimum)
        let velocity_score = match velocity_sol_per_sec {
            // Unmeasured — neither evidence for nor against. Neutral keeps the
            // component from acting as a flat tax on every candidate.
            None => 0.5,
            Some(v) if v >= PHI * 2.0 => 1.0, // Exceptional: > 3.2 SOL/s
            Some(v) if v >= PHI => 0.8,       // Strong: > 1.618 SOL/s
            Some(v) if v >= 1.0 => 0.5,       // Moderate
            Some(_) => 0.2,                   // Weak — measured, and genuinely slow
        };
        score += velocity_score * 0.25;

        // Buy pressure: Golden ratio or higher indicates strong demand
        let pressure_score = if buy_pressure_ratio >= PHI {
            1.0 // Exceeds golden ratio
        } else if buy_pressure_ratio >= 1.0 {
            0.7 // Above 1:1
        } else if buy_pressure_ratio >= PHI_INV {
            0.4 // Above inverse golden ratio
        } else {
            0.1 // Weak pressure
        };
        score += pressure_score * 0.10;

        score.max(0.0).min(1.0)
    }
}

/// Signal from Fibonacci momentum analysis
#[derive(Debug, Clone)]
pub enum FibonacciSignal {
    /// Hold position - momentum still building
    Hold {
        gain_pct: f64,
        velocity_ratio: f64,
        next_target_pct: f64,
    },
    /// Runner detected - strong Fibonacci velocity pattern
    RunnerDetected { velocity_ratio: f64, age_secs: u64 },
    /// Escalate TP to next Fibonacci level
    EscalateToNextFib {
        current_gain_pct: f64,
        next_target_pct: f64,
        velocity_ratio: f64,
    },
    /// Take profit at current Fibonacci level
    TakeProfitAtFib { gain_pct: f64, fib_level: usize },
    /// Exit on golden retracement (61.8% pullback from peak)
    ExitGoldenRetrace {
        peak_gain_pct: f64,
        current_gain_pct: f64,
        retrace_pct: f64,
    },
    /// Exit on velocity collapse at retracement level
    ExitVelocityCollapse {
        peak_gain_pct: f64,
        current_gain_pct: f64,
        velocity_ratio: f64,
    },
}

impl FibonacciSignal {
    /// Check if this signal recommends exiting the position
    pub fn should_exit(&self) -> bool {
        matches!(
            self,
            FibonacciSignal::TakeProfitAtFib { .. }
                | FibonacciSignal::ExitGoldenRetrace { .. }
                | FibonacciSignal::ExitVelocityCollapse { .. }
        )
    }

    /// Get human-readable description of the signal
    pub fn description(&self) -> String {
        match self {
            FibonacciSignal::Hold {
                gain_pct,
                velocity_ratio,
                next_target_pct,
            } => {
                format!(
                    "HOLD: +{:.1}% gain, velocity ratio {:.2}, next Fib target {:.0}%",
                    gain_pct, velocity_ratio, next_target_pct
                )
            }
            FibonacciSignal::RunnerDetected {
                velocity_ratio,
                age_secs,
            } => {
                format!(
                    "🚀 RUNNER: Fibonacci velocity {:.2}φ detected at {}s",
                    velocity_ratio / PHI,
                    age_secs
                )
            }
            FibonacciSignal::EscalateToNextFib {
                current_gain_pct,
                next_target_pct,
                velocity_ratio,
            } => {
                format!(
                    "📈 ESCALATE: +{:.1}% → {:.0}% target (velocity {:.2}φ)",
                    current_gain_pct,
                    next_target_pct,
                    velocity_ratio / PHI
                )
            }
            FibonacciSignal::TakeProfitAtFib {
                gain_pct,
                fib_level,
            } => {
                format!(
                    "💰 TAKE PROFIT: +{:.1}% at Fibonacci level {}",
                    gain_pct, fib_level
                )
            }
            FibonacciSignal::ExitGoldenRetrace {
                peak_gain_pct,
                current_gain_pct,
                retrace_pct,
            } => {
                format!(
                    "🔻 GOLDEN RETRACE: Peak +{:.1}% → +{:.1}% ({:.1}% pullback)",
                    peak_gain_pct, current_gain_pct, retrace_pct
                )
            }
            FibonacciSignal::ExitVelocityCollapse {
                peak_gain_pct,
                current_gain_pct,
                velocity_ratio,
            } => {
                format!(
                    "⚠️ VELOCITY COLLAPSE: Peak +{:.1}% → +{:.1}% (ratio {:.2})",
                    peak_gain_pct, current_gain_pct, velocity_ratio
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fibonacci_constants() {
        // Verify golden ratio properties
        assert!((PHI - 1.618).abs() < 0.001);
        assert!((PHI_INV - 0.618).abs() < 0.001);
        assert!((PHI * PHI_INV - 1.0).abs() < 0.001); // φ × (1/φ) = 1
    }

    #[test]
    fn test_runner_detection() {
        let entry = 10_000_000; // 0.01 SOL
        let mut momentum = FibonacciMomentum::new(entry, 1000);

        // Simulate strong velocity in first 5 seconds
        let signal = momentum.update(entry * 2, 1005, 15_000_000_000);

        match signal {
            FibonacciSignal::RunnerDetected { .. } => {
                // Expected: strong velocity + early age = runner
            }
            _ => panic!("Should detect runner with strong early velocity"),
        }
    }

    #[test]
    fn test_golden_retracement_exit() {
        let entry = 10_000_000;
        let mut momentum = FibonacciMomentum::new(entry, 1000);

        // Pump to 3x
        momentum.update(entry * 3, 1010, 20_000_000_000);

        // Retrace 61.8% from peak
        let retrace_value = entry + ((entry * 2) as f64 * (1.0 - FIB_RETRACE_61_8)) as u64;
        let signal = momentum.update(retrace_value, 1020, 18_000_000_000);

        assert!(signal.should_exit(), "Should exit on golden retracement");
    }

    #[test]
    fn test_fibonacci_position_sizing() {
        assert_eq!(
            FibonacciMomentum::calculate_position_multiplier(0, 1.0),
            1.0
        );
        assert_eq!(
            FibonacciMomentum::calculate_position_multiplier(3, 1.0),
            2.0
        );
        assert_eq!(
            FibonacciMomentum::calculate_position_multiplier(5, 1.0),
            5.0
        );
        assert_eq!(
            FibonacciMomentum::calculate_position_multiplier(7, 1.0),
            13.0
        );
        assert_eq!(
            FibonacciMomentum::calculate_position_multiplier(10, 1.0),
            21.0
        ); // Capped
    }

    #[test]
    fn test_pool_fibonacci_scoring() {
        // Perfect Fibonacci pool: 13 SOL, 3s old, 2.5 SOL/s velocity, 1.8 pressure
        let score = FibonacciMomentum::score_pool_fibonacci(13.0, 3, Some(2.5), 1.8);
        assert!(
            score > 0.85,
            "Perfect Fibonacci pool should score high: {}",
            score
        );

        // Poor pool: 100 SOL, 60s old, 0.1 SOL/s, 0.2 pressure
        let score = FibonacciMomentum::score_pool_fibonacci(100.0, 60, Some(0.1), 0.2);
        assert!(score < 0.3, "Poor pool should score low: {}", score);
    }

    /// An unmeasurable velocity must not be scored as a slow one.
    ///
    /// This is the regression for the bug that stalled the sniper: velocity arrived as
    /// a literal 0.0 for every pool, so every candidate was docked the same 0.05 and
    /// the gate stopped discriminating on anything but size.
    #[test]
    fn unmeasured_velocity_scores_neutral_not_weak() {
        let unmeasured = FibonacciMomentum::score_pool_fibonacci(13.0, 3, None, 1.8);
        let measured_slow = FibonacciMomentum::score_pool_fibonacci(13.0, 3, Some(0.1), 1.8);
        let measured_fast = FibonacciMomentum::score_pool_fibonacci(13.0, 3, Some(3.5), 1.8);

        assert!(
            unmeasured > measured_slow,
            "absent evidence must not be penalised like a measured stall: {} vs {}",
            unmeasured,
            measured_slow
        );
        assert!(
            unmeasured < measured_fast,
            "absent evidence must not be rewarded like a measured surge: {} vs {}",
            unmeasured,
            measured_fast
        );
    }

    /// The exact shape that produced 519 of 543 gate rejections in the decision log.
    ///
    /// With velocity unmeasurable and buy pressure on its raw (unnormalised) scale, a
    /// pool in the Fibonacci size range must clear a 0.50 entry threshold, while one
    /// outside the range must still be turned away. If both pass, the gate has become a
    /// no-op and the bot is buying indiscriminately — which is the failure mode on the
    /// other side of this fix.
    #[test]
    fn detection_time_pool_is_graded_by_size_not_blocked_wholesale() {
        // 12 SOL: inside the 8-21 SOL sweet spot.
        let sweet_spot = FibonacciMomentum::score_pool_fibonacci(12.0, 0, None, 0.0001);
        // 400 SOL: far outside any Fibonacci band.
        let oversized = FibonacciMomentum::score_pool_fibonacci(400.0, 0, None, 0.0001);

        assert!(
            sweet_spot >= 0.50,
            "a sweet-spot pool must clear the 0.50 gate at detection time, got {}",
            sweet_spot
        );
        assert!(
            oversized < 0.50,
            "an oversized pool must still be rejected, got {}",
            oversized
        );
    }
}
