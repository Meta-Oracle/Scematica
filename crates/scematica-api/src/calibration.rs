//! Calibration — score Scylar's past claims against what the mints actually did.
//!
//! The unusual property this exploits: **ground truth arrives automatically, minutes
//! later.** She says a pool looks strong; `scematica-trades.jsonl` records what it did.
//! Most assistants cannot be scored because nothing ever confirms them. Here the
//! confirmation is a file the bot is already writing.
//!
//! That makes her trustworthy in a *measured* way rather than a stylistic one — "of the
//! 40 pools I called strong, 12 rugged" is a fact about her, not a tone.
//!
//! # Two limits, both load-bearing
//!
//! **Claims are scoped to the sentence that names the mint**, not to the message. A
//! paragraph mentioning four mints does not hold one opinion; attributing the message's
//! overall sentiment to every mint in it would manufacture claims she never made and
//! then score her on them.
//!
//! **Only claims with an outcome are scored.** A bullish call is resolvable — if the bot
//! bought, there is realised PnL. A bearish call usually is not: nobody buys what she
//! warns against, so nothing records whether the warning was right. That asymmetry is
//! reported, never closed with an estimate. Scoring an assistant on outcomes it caused
//! to not happen is how a calibration number becomes flattery.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Solana mints are base58 and land in this length range; ordinary words never do.
const MINT_MIN_LEN: usize = 32;
const MINT_MAX_LEN: usize = 44;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Stance {
    Bullish,
    Bearish,
    Neutral,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub mint: String,
    pub stance: Stance,
    /// Unix seconds when the claim was made.
    pub at: u64,
    /// The sentence it came from, so a scored claim can be read back and disputed.
    pub context: String,
}

#[derive(Debug, Default, Serialize)]
pub struct Calibration {
    pub claims_total: usize,
    pub bullish: usize,
    pub bearish: usize,

    /// Bullish calls on mints that were actually traded — the scoreable set.
    pub bullish_resolved: usize,
    pub bullish_correct: usize,
    pub bullish_wrong: usize,
    /// Correct / resolved. `None` when nothing has resolved yet — not 0.0, which would
    /// read as "always wrong".
    pub bullish_accuracy: Option<f64>,
    /// Realised SOL across the mints she was bullish on.
    pub bullish_realised_pnl_sol: f64,

    /// Bearish calls the bot did buy anyway — the only bearish ones with an outcome.
    pub bearish_resolved: usize,
    pub bearish_correct: usize,
    pub bearish_wrong: usize,

    /// Claims with no outcome on record.
    pub unresolved: usize,
    pub notes: Vec<String>,
}

fn is_base58(c: char) -> bool {
    c.is_ascii_alphanumeric() && !matches!(c, '0' | 'O' | 'I' | 'l')
}

/// Mint-shaped tokens in a string.
pub fn extract_mints(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for token in text.split(|c: char| !c.is_ascii_alphanumeric()) {
        if (MINT_MIN_LEN..=MINT_MAX_LEN).contains(&token.len()) && token.chars().all(is_base58) {
            let t = token.to_string();
            if !out.contains(&t) {
                out.push(t);
            }
        }
    }
    out
}

const BEARISH: &[&str] = &[
    "rug", "avoid", "risky", "risk", "weak", "dump", "suspicious", "skip", "dangerous",
    "thin", "shallow", "concentrated", "not worth", "stay away", "would not", "wouldn't",
    "bearish", "trap", "drain",
];

const BULLISH: &[&str] = &[
    "strong", "healthy", "promising", "solid", "clean", "worth", "runner", "good entry",
    "looks good", "bullish", "deep", "burned", "renounced",
];

/// Classify one sentence.
///
/// Bearish wins ties, and is checked first, for the same reason the avatar's sentiment
/// check is: "strong buy pressure but the LP is not burned — avoid" is a warning, and a
/// tie broken the other way would score her as bullish on a pool she told you to skip.
pub fn classify(sentence: &str) -> Stance {
    let s = sentence.to_lowercase();
    if BEARISH.iter().any(|w| s.contains(w)) {
        return Stance::Bearish;
    }
    if BULLISH.iter().any(|w| s.contains(w)) {
        return Stance::Bullish;
    }
    Stance::Neutral
}

/// Pull claims out of an answer, one per (sentence, mint) pair.
///
/// Neutral mentions are dropped rather than stored: "the pool is 40 SOL deep" names a
/// mint without making a claim about it, and counting it would pad the denominator with
/// statements that cannot be right or wrong.
pub fn extract_claims(text: &str, at: u64) -> Vec<Claim> {
    let mut claims = Vec::new();
    for sentence in text.split(['.', '!', '?', '\n']) {
        let trimmed = sentence.trim();
        if trimmed.is_empty() {
            continue;
        }
        let stance = classify(trimmed);
        if stance == Stance::Neutral {
            continue;
        }
        for mint in extract_mints(trimmed) {
            claims.push(Claim {
                mint,
                stance,
                at,
                context: trimmed.chars().take(200).collect(),
            });
        }
    }
    claims
}

/// Score claims against realised PnL per mint.
pub fn score(claims: &[Claim], pnl: &HashMap<String, f64>) -> Calibration {
    let mut c = Calibration { claims_total: claims.len(), ..Default::default() };

    for claim in claims {
        let outcome = pnl.get(&claim.mint).copied();
        match claim.stance {
            Stance::Bullish => {
                c.bullish += 1;
                match outcome {
                    Some(p) => {
                        c.bullish_resolved += 1;
                        c.bullish_realised_pnl_sol += p;
                        if p > 0.0 {
                            c.bullish_correct += 1;
                        } else {
                            c.bullish_wrong += 1;
                        }
                    }
                    None => c.unresolved += 1,
                }
            }
            Stance::Bearish => {
                c.bearish += 1;
                match outcome {
                    // Bought despite the warning — the rare case where a bearish call is
                    // testable at all.
                    Some(p) => {
                        c.bearish_resolved += 1;
                        if p <= 0.0 {
                            c.bearish_correct += 1;
                        } else {
                            c.bearish_wrong += 1;
                        }
                    }
                    None => c.unresolved += 1,
                }
            }
            Stance::Neutral => {}
        }
    }

    if c.bullish_resolved > 0 {
        c.bullish_accuracy = Some(c.bullish_correct as f64 / c.bullish_resolved as f64);
    }

    if c.unresolved > 0 {
        c.notes.push(format!(
            "{} claims have no outcome on record — mostly warnings about pools the bot \
             never bought, so nothing can confirm or refute them. They are counted, not \
             scored, and no accuracy figure includes them.",
            c.unresolved
        ));
    }
    if c.bullish_resolved == 0 {
        c.notes.push(
            "No bullish call has resolved yet, so there is no accuracy figure to report."
                .to_string(),
        );
    }
    if c.bearish > 0 && c.bearish_resolved == 0 {
        c.notes.push(
            "Every bearish call is unresolved: the bot avoided those pools, so being \
             right cost nothing and being wrong left no trace. Do not read the absence \
             of losses as vindication."
                .to_string(),
        );
    }

    c
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINT_A: &str = "vriFfbqmSyxeYaqzGrCg3rMdXL1vPaVcApAC9Grpump";
    const MINT_B: &str = "C47scuyDpx36kRzWKvd9992PaVcApAC9GrCg3rMdXL1v";

    fn pnl(pairs: &[(&str, f64)]) -> HashMap<String, f64> {
        pairs.iter().map(|(m, p)| (m.to_string(), *p)).collect()
    }

    #[test]
    fn mints_are_found_and_words_are_not() {
        let found = extract_mints(&format!("The pool {MINT_A} looks deep and healthy today"));
        assert_eq!(found, vec![MINT_A.to_string()]);
    }

    #[test]
    fn a_mint_is_not_double_counted_in_one_sentence() {
        assert_eq!(extract_mints(&format!("{MINT_A} and again {MINT_A}")).len(), 1);
    }

    #[test]
    fn claims_are_scoped_to_their_sentence() {
        // Two mints, two opposite opinions, one message. Attributing one sentiment to
        // both is the failure this exists to prevent.
        let text = format!("{MINT_A} looks strong and healthy. {MINT_B} is a likely rug, avoid it.");
        let claims = extract_claims(&text, 0);
        assert_eq!(claims.len(), 2);
        let a = claims.iter().find(|c| c.mint == MINT_A).unwrap();
        let b = claims.iter().find(|c| c.mint == MINT_B).unwrap();
        assert_eq!(a.stance, Stance::Bullish);
        assert_eq!(b.stance, Stance::Bearish);
    }

    #[test]
    fn a_hedged_recommendation_counts_as_a_warning() {
        assert_eq!(classify("strong buy pressure but the LP is not burned — avoid"), Stance::Bearish);
    }

    #[test]
    fn a_neutral_mention_is_not_a_claim() {
        let claims = extract_claims(&format!("The pool {MINT_A} is 40 SOL and 12 seconds old"), 0);
        assert!(claims.is_empty());
    }

    #[test]
    fn bullish_calls_are_scored_against_realised_pnl() {
        let claims = extract_claims(
            &format!("{MINT_A} looks strong. {MINT_B} looks solid."),
            0,
        );
        let c = score(&claims, &pnl(&[(MINT_A, 0.4), (MINT_B, -0.3)]));
        assert_eq!(c.bullish_resolved, 2);
        assert_eq!(c.bullish_correct, 1);
        assert_eq!(c.bullish_wrong, 1);
        assert_eq!(c.bullish_accuracy, Some(0.5));
        assert!((c.bullish_realised_pnl_sol - 0.1).abs() < 1e-9);
    }

    #[test]
    fn unresolved_claims_are_counted_never_scored() {
        let claims = extract_claims(&format!("{MINT_A} is a likely rug, avoid."), 0);
        let c = score(&claims, &pnl(&[]));
        assert_eq!(c.bearish, 1);
        assert_eq!(c.bearish_resolved, 0);
        assert_eq!(c.unresolved, 1);
        assert!(c.notes.iter().any(|n| n.contains("vindication")));
    }

    #[test]
    fn no_resolutions_yields_no_accuracy_rather_than_zero() {
        let c = score(&extract_claims(&format!("{MINT_A} looks strong."), 0), &pnl(&[]));
        // 0.0 would render as "always wrong"; absence is the honest value.
        assert_eq!(c.bullish_accuracy, None);
    }

    #[test]
    fn a_bearish_call_the_bot_overrode_is_scoreable() {
        let claims = extract_claims(&format!("{MINT_A} looks like a rug."), 0);
        let c = score(&claims, &pnl(&[(MINT_A, -0.5)]));
        assert_eq!(c.bearish_resolved, 1);
        assert_eq!(c.bearish_correct, 1);
    }
}
