//! Real routing + execution backed by the Jupiter v6 aggregator and the
//! Scematica Deep Q* agent.
//!
//! [`JupiterRoutePolicy`] replaces the SDK's stub policy with **real quotes** —
//! `expected_out` comes from Jupiter, so the Conviction-Routing bond is sized
//! against an actual fill estimate instead of a placeholder. The DQ* value head
//! supplies conviction, discounted by the quote's price impact, and
//! [`JupiterRoutePolicy::observe_outcome`] closes the reinforcement loop so the
//! agent learns from realized fills vs. its bonded promise.

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::anyhow;
use async_trait::async_trait;
use scematica_executor::jupiter::JupiterBuilder;
use scematica_nn::{DQNAgent, TradeAction, TradeState};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcTransactionConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature, Signer};
use solana_sdk::transaction::VersionedTransaction;
use solana_transaction_status::option_serializer::OptionSerializer;
use solana_transaction_status::{UiTransactionEncoding, UiTransactionTokenBalance};

use scemadex_sdk::{
    Amount, Conviction, Fill, Intent, Result, Route, RouteLeg, RoutePolicy, ScemaDexError,
    Solution, SwapInstructions, Venue, VenueExecutor,
};

/// Parsed essentials of a Jupiter v6 quote.
pub struct ParsedQuote {
    /// Guaranteed-ish output in the output mint's base units.
    pub out_amount: u64,
    /// Price impact as a fraction (e.g. `0.0012` = 0.12%).
    pub price_impact_pct: f64,
    /// Venue labels of each hop in the chosen route.
    pub labels: Vec<String>,
}

/// Parse the fields we need from a Jupiter `/quote` response. Pure + offline.
pub fn parse_jupiter_quote(v: &serde_json::Value) -> anyhow::Result<ParsedQuote> {
    let out_amount = v["outAmount"]
        .as_str()
        .ok_or_else(|| anyhow!("quote missing outAmount"))?
        .parse::<u64>()?;
    let price_impact_pct = v["priceImpactPct"]
        .as_str()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(0.0);
    let labels = v["routePlan"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|p| p["swapInfo"]["label"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Ok(ParsedQuote {
        out_amount,
        price_impact_pct,
        labels,
    })
}

fn owner_matches(field: &OptionSerializer<String>, owner: &str) -> bool {
    matches!(field, OptionSerializer::Some(o) if o == owner)
}

/// Realized output amount received by `owner` for `out_mint`, computed from a
/// transaction's pre/post SPL-token balances (the mint's base units).
///
/// Returns the positive post−pre delta on the owner's `out_mint` account, or
/// `None` when the balances don't contain a post entry for that owner+mint
/// (e.g. the output is native SOL that Jupiter unwrapped, so no token balance
/// survives — callers fall back to the quoted amount). The pre entry is treated
/// as zero when absent, since the output ATA is often created during the swap.
/// Pure + offline.
pub fn realized_out_from_balances(
    pre: &[UiTransactionTokenBalance],
    post: &[UiTransactionTokenBalance],
    owner: &str,
    out_mint: &str,
) -> Option<u64> {
    let amount_of = |bals: &[UiTransactionTokenBalance]| -> Option<u64> {
        bals.iter()
            .find(|b| b.mint == out_mint && owner_matches(&b.owner, owner))
            .and_then(|b| b.ui_token_amount.amount.parse::<u64>().ok())
    };
    let post_amt = amount_of(post)?;
    let pre_amt = amount_of(pre).unwrap_or(0);
    Some(post_amt.saturating_sub(pre_amt))
}

/// Fetch a confirmed transaction and extract the realized output amount for
/// `owner`/`out_mint` from its token-balance meta. Best-effort: any RPC error,
/// missing meta, or unparseable balance yields `None` so the caller can fall
/// back to the quoted amount.
async fn fetch_realized_out(
    rpc: &RpcClient,
    sig: &Signature,
    owner: &str,
    out_mint: &str,
) -> Option<u64> {
    let cfg = RpcTransactionConfig {
        encoding: Some(UiTransactionEncoding::JsonParsed),
        commitment: Some(CommitmentConfig::confirmed()),
        max_supported_transaction_version: Some(0),
    };
    let tx = rpc.get_transaction_with_config(sig, cfg).await.ok()?;
    let meta = tx.transaction.meta?;
    let (OptionSerializer::Some(pre), OptionSerializer::Some(post)) =
        (meta.pre_token_balances, meta.post_token_balances)
    else {
        return None;
    };
    realized_out_from_balances(&pre, &post, owner, out_mint)
}

/// Map a Jupiter venue label to the SDK [`Venue`].
pub fn venue_from_label(label: Option<&str>) -> Venue {
    match label {
        Some(l) if l.eq_ignore_ascii_case("Raydium") => Venue::Raydium,
        Some(l) if l.contains("Orca") || l.contains("Whirlpool") => Venue::Orca,
        Some(l) if l.contains("Meteora") => Venue::Meteora,
        Some(l) if !l.is_empty() => Venue::Custom,
        _ => Venue::Jupiter,
    }
}

/// A [`RoutePolicy`] that quotes Jupiter for real and sizes conviction with DQ*.
pub struct JupiterRoutePolicy {
    jup: JupiterBuilder,
    agent: Option<Mutex<DQNAgent>>,
    /// Decisions awaiting an outcome, keyed by intent digest, for the reward loop.
    pending: Mutex<HashMap<String, (TradeState, TradeAction)>>,
}

impl JupiterRoutePolicy {
    /// Quote-only policy: conviction derives from price impact alone.
    pub fn new() -> Self {
        Self {
            jup: JupiterBuilder::new(),
            agent: None,
            pending: Mutex::new(HashMap::new()),
        }
    }

    /// Attach a Deep Q* agent so conviction and the reward loop are learned.
    pub fn with_agent(mut self, agent: DQNAgent) -> Self {
        self.agent = Some(Mutex::new(agent));
        self
    }

    /// Attach the bot's trained agent from a checkpoint (e.g. the sniper's
    /// `scematica-nn-agent.json`). Falls back to quote-only conviction if the
    /// checkpoint is missing or unreadable. Lets consumers wire the trained
    /// policy without depending on `scematica-nn` directly.
    pub fn with_agent_from_checkpoint(self, path: &str) -> Self {
        match DQNAgent::load(path) {
            Ok(agent) => self.with_agent(agent),
            Err(_) => self,
        }
    }

    fn state_for(intent: &Intent, price_impact: f64) -> TradeState {
        TradeState {
            initial_liquidity_sol: intent.amount_in.ui(),
            spread_pct: price_impact,
            deployer_rug_rate: 0.5,
            ..Default::default()
        }
    }

    /// Conviction in `[0,1]`: DQ* confidence (if present) discounted by price
    /// impact (up to a 50% haircut at ≥5% impact). Records the chosen action for
    /// the reward loop.
    fn conviction_for(&self, intent: &Intent, price_impact: f64) -> f64 {
        let state = Self::state_for(intent, price_impact);
        let base = if let Some(agent) = &self.agent {
            if let Ok(mut ag) = agent.lock() {
                let (action, q) = ag.advise(&state);
                if let Ok(mut pending) = self.pending.lock() {
                    pending.insert(intent.digest(), (state.clone(), action));
                }
                let sum_abs: f64 = q.iter().map(|v| v.abs()).sum();
                let best = q
                    .iter()
                    .copied()
                    .filter(|v| v.is_finite())
                    .fold(f64::NEG_INFINITY, f64::max);
                if sum_abs > 1e-9 && best.is_finite() {
                    (best.abs() / sum_abs).clamp(0.0, 1.0)
                } else {
                    0.5
                }
            } else {
                0.5
            }
        } else {
            0.6
        };
        let impact_haircut = (price_impact.min(0.05) / 0.05) * 0.5;
        (base * (1.0 - impact_haircut)).clamp(0.0, 1.0)
    }

    /// Close the reinforcement loop after a fill settles. Reward = realized vs.
    /// expected output (in %), minus a penalty when the bond was slashed.
    pub fn observe_outcome(
        &self,
        intent: &Intent,
        expected_out_raw: u64,
        fill_out_raw: u64,
        slashed: bool,
    ) {
        let Some(agent) = &self.agent else { return };
        let decision = self
            .pending
            .lock()
            .ok()
            .and_then(|mut p| p.remove(&intent.digest()));
        let Some((state, action)) = decision else {
            return;
        };

        let ratio = fill_out_raw as f64 / expected_out_raw.max(1) as f64;
        let mut reward = (ratio - 1.0) * 100.0;
        if slashed {
            reward -= 50.0;
        }
        if let Ok(mut ag) = agent.lock() {
            // Single terminal transition: the route either delivered or it didn't.
            ag.observe(state.clone(), action, reward, state, true);
        }
    }
}

impl Default for JupiterRoutePolicy {
    fn default() -> Self {
        Self::new()
    }
}

fn mint(addr: &scemadex_sdk::Address) -> Result<Pubkey> {
    Pubkey::from_str(addr.as_str())
        .map_err(|_| ScemaDexError::Other(format!("invalid mint: {}", addr.as_str())))
}

#[async_trait]
impl RoutePolicy for JupiterRoutePolicy {
    async fn solve(&self, intent: &Intent) -> Result<Solution> {
        let in_mint = mint(&intent.input_mint)?;
        let out_mint = mint(&intent.output_mint)?;
        let slippage = intent.constraints.max_slippage_bps.min(u16::MAX as u32) as u16;

        let quote = self
            .jup
            .get_quote(&in_mint, &out_mint, intent.amount_in.raw, slippage)
            .await
            .map_err(|e| ScemaDexError::NoRoute(format!("jupiter quote: {e}")))?;
        let parsed =
            parse_jupiter_quote(&quote).map_err(|e| ScemaDexError::NoRoute(e.to_string()))?;

        // Represent Jupiter's (possibly multi-hop) path as a single leg whose
        // expected_out is the real quoted amount; hop labels go in the rationale.
        // Output decimals are unknown from the quote, so `raw` is authoritative.
        let expected_out = Amount::new(parsed.out_amount, 0);
        let leg = RouteLeg {
            venue: venue_from_label(parsed.labels.first().map(|s| s.as_str())),
            input_mint: intent.input_mint.clone(),
            output_mint: intent.output_mint.clone(),
            split_bps: 10_000,
            expected_out,
        };
        let conviction = self.conviction_for(intent, parsed.price_impact_pct);

        Ok(Solution {
            intent_digest: intent.digest(),
            route: Route {
                legs: vec![leg],
                expected_out,
            },
            conviction: Conviction::clamped(conviction),
            rationale: format!(
                "Jupiter route [{}] out={} impact={:.4}%",
                parsed.labels.join(" → "),
                parsed.out_amount,
                parsed.price_impact_pct * 100.0
            ),
        })
    }
}

/// A [`VenueExecutor`] that builds real Jupiter swap transactions. In dry mode
/// it returns a freshly-quoted fill; in live mode (signer + RPC wired via
/// [`JupiterVenueExecutor::with_signer`]) it submits the swap and reports the
/// **realized** output parsed from the confirmed transaction's token-balance
/// meta, falling back to the quoted amount only if the tx can't be read.
pub struct JupiterVenueExecutor {
    jup: JupiterBuilder,
    /// Owner pubkey the swap transaction is built for.
    owner: Pubkey,
    slippage_bps: u16,
    /// When present, `execute()` signs and submits on-chain instead of dry-quoting.
    signer: Option<Arc<Keypair>>,
    rpc: Option<Arc<RpcClient>>,
}

impl JupiterVenueExecutor {
    pub fn new(owner: Pubkey) -> Self {
        Self {
            jup: JupiterBuilder::new(),
            owner,
            slippage_bps: 150,
            signer: None,
            rpc: None,
        }
    }

    /// Enable live submission: `execute()` will sign the Jupiter transaction with
    /// `signer` and `send_and_confirm` it via `rpc`. The owner is set to the
    /// signer's pubkey so the built transaction matches.
    pub fn with_signer(mut self, signer: Arc<Keypair>, rpc: Arc<RpcClient>) -> Self {
        self.owner = signer.pubkey();
        self.signer = Some(signer);
        self.rpc = Some(rpc);
        self
    }

    pub fn with_slippage_bps(mut self, bps: u16) -> Self {
        self.slippage_bps = bps;
        self
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[async_trait]
impl VenueExecutor for JupiterVenueExecutor {
    async fn build(&self, route: &Route) -> Result<SwapInstructions> {
        let leg = route
            .legs
            .first()
            .ok_or_else(|| ScemaDexError::Venue("empty route".into()))?;
        let in_mint = mint(&leg.input_mint)?;
        let out_mint = mint(&leg.output_mint)?;
        // Re-quote for the full order, then fetch the swap transaction bytes.
        let total_in = route_input_amount(route);
        let quote = self
            .jup
            .get_quote(&in_mint, &out_mint, total_in, self.slippage_bps)
            .await
            .map_err(|e| ScemaDexError::Venue(format!("jupiter quote: {e}")))?;
        let tx = self
            .jup
            .get_swap_transaction(&quote, &self.owner)
            .await
            .map_err(|e| ScemaDexError::Venue(format!("jupiter swap: {e}")))?;
        Ok(SwapInstructions {
            serialized: tx,
            describe: format!("jupiter swap for {}", self.owner),
        })
    }

    async fn execute(&self, route: &Route) -> Result<Fill> {
        let leg = route
            .legs
            .first()
            .ok_or_else(|| ScemaDexError::Venue("empty route".into()))?;
        let in_mint = mint(&leg.input_mint)?;
        let out_mint = mint(&leg.output_mint)?;
        let total_in = route_input_amount(route);
        let quote = self
            .jup
            .get_quote(&in_mint, &out_mint, total_in, self.slippage_bps)
            .await
            .map_err(|e| ScemaDexError::Venue(format!("jupiter quote: {e}")))?;
        let parsed =
            parse_jupiter_quote(&quote).map_err(|e| ScemaDexError::Venue(e.to_string()))?;

        // Live mode: fetch the swap tx, re-sign it with our keypair, submit, confirm.
        if let (Some(signer), Some(rpc)) = (&self.signer, &self.rpc) {
            let tx_bytes = self
                .jup
                .get_swap_transaction(&quote, &self.owner)
                .await
                .map_err(|e| ScemaDexError::Venue(format!("jupiter swap: {e}")))?;
            let unsigned = self
                .jup
                .deserialize_transaction(&tx_bytes)
                .map_err(|e| ScemaDexError::Venue(format!("decode swap tx: {e}")))?;
            let signed = VersionedTransaction::try_new(unsigned.message, &[signer.as_ref()])
                .map_err(|e| ScemaDexError::Venue(format!("sign swap tx: {e}")))?;
            let sig = rpc
                .send_and_confirm_transaction(&signed)
                .await
                .map_err(|e| ScemaDexError::Venue(format!("submit swap: {e}")))?;
            // Prefer the realized fill parsed from the confirmed tx's token-balance
            // meta; fall back to the quoted amount if the tx can't be read.
            let owner = self.owner.to_string();
            let realized = fetch_realized_out(rpc, &sig, &owner, leg.output_mint.as_str()).await;
            let amount_out = realized.unwrap_or(parsed.out_amount);
            return Ok(Fill {
                amount_out: Amount::new(amount_out, 0),
                executed_unix: unix_now(),
            });
        }

        // Dry mode: report the quoted output as the fill (no submission).
        Ok(Fill {
            amount_out: Amount::new(parsed.out_amount, 0),
            executed_unix: 0,
        })
    }
}

/// Best-effort recovery of the order's input amount. The SDK `Route` only
/// carries expected output, so we fall back to the expected_out as a proxy when
/// no better signal is available; callers with the original intent should prefer
/// `JupiterVenueExecutor::build` driven by that amount.
fn route_input_amount(route: &Route) -> u64 {
    route.expected_out.raw.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_hop_quote() {
        let v: serde_json::Value = serde_json::from_str(
            r#"{"outAmount":"123456","priceImpactPct":"0.0012",
                "routePlan":[{"swapInfo":{"label":"Raydium"},"percent":100}]}"#,
        )
        .unwrap();
        let p = parse_jupiter_quote(&v).unwrap();
        assert_eq!(p.out_amount, 123_456);
        assert!((p.price_impact_pct - 0.0012).abs() < 1e-9);
        assert_eq!(p.labels, vec!["Raydium".to_string()]);
    }

    #[test]
    fn maps_venue_labels() {
        assert!(matches!(venue_from_label(Some("Raydium")), Venue::Raydium));
        assert!(matches!(
            venue_from_label(Some("Orca (Whirlpool)")),
            Venue::Orca
        ));
        assert!(matches!(
            venue_from_label(Some("Meteora DLMM")),
            Venue::Meteora
        ));
        assert!(matches!(venue_from_label(Some("Lifinity")), Venue::Custom));
        assert!(matches!(venue_from_label(None), Venue::Jupiter));
    }

    fn bal(mint: &str, owner: &str, amount: &str) -> UiTransactionTokenBalance {
        use solana_account_decoder::parse_token::UiTokenAmount;
        UiTransactionTokenBalance {
            account_index: 0,
            mint: mint.to_string(),
            ui_token_amount: UiTokenAmount {
                ui_amount: None,
                decimals: 0,
                amount: amount.to_string(),
                ui_amount_string: amount.to_string(),
            },
            owner: OptionSerializer::Some(owner.to_string()),
            program_id: OptionSerializer::None,
        }
    }

    #[test]
    fn realized_out_takes_owner_mint_delta() {
        let owner = "OwnerPubkey1111111111111111111111111111111";
        let out = "OutMint11111111111111111111111111111111111";
        // ATA created during the swap: no pre entry, post = 5000.
        let pre = vec![bal("OtherMint", owner, "42")];
        let post = vec![bal(out, owner, "5000"), bal("OtherMint", owner, "42")];
        assert_eq!(
            realized_out_from_balances(&pre, &post, owner, out),
            Some(5000)
        );
    }

    #[test]
    fn realized_out_subtracts_existing_balance() {
        let owner = "OwnerPubkey1111111111111111111111111111111";
        let out = "OutMint11111111111111111111111111111111111";
        let pre = vec![bal(out, owner, "1000")];
        let post = vec![bal(out, owner, "3500")];
        assert_eq!(
            realized_out_from_balances(&pre, &post, owner, out),
            Some(2500)
        );
    }

    #[test]
    fn realized_out_none_when_owner_absent() {
        let owner = "OwnerPubkey1111111111111111111111111111111";
        let out = "OutMint11111111111111111111111111111111111";
        // Output unwrapped to native SOL → no surviving token balance for owner.
        let post = vec![bal(out, "SomeoneElse", "5000")];
        assert_eq!(realized_out_from_balances(&[], &post, owner, out), None);
    }

    #[tokio::test]
    async fn conviction_discounts_for_price_impact() {
        let policy = JupiterRoutePolicy::new();
        // No agent → base 0.6; 5% impact → 50% haircut → 0.30.
        let intent = scemadex_sdk::demo_intent();
        let c_lo = policy.conviction_for(&intent, 0.0);
        let c_hi = policy.conviction_for(&intent, 0.05);
        assert!(c_lo > c_hi);
        assert!((c_hi - 0.30).abs() < 1e-6);
    }
}
