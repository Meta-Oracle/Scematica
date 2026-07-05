//! # scemadex-settle — open devnet reference settler
//!
//! The published [`scemadex_sdk`] ships the Conviction-Routing **settlement state
//! machine** ([`scemadex_sdk::EscrowBondEngine`]) but deliberately moves no money
//! — it carries no `solana-sdk` dependency. This crate closes that loop on
//! **devnet**: it wraps the state machine and, when a bond is **slashed**, makes a
//! real on-chain SPL-USDC transfer of the bond amount to the caller.
//!
//! It exists so external developers can run the *entire* Conviction-Routing loop —
//! quote → bond → execute → settle on-chain — for free, with no proprietary stack
//! and no funds at risk. The production mainnet rail (x402 metering, fee
//! abstraction, the trust/relay network) is a separate, closed component.
//!
//! ## ⚠️ Devnet / test only
//!
//! This is a *reference* implementation. It performs a plain SPL transfer of the
//! bond on slash; it does **not** implement escrow custody, fee collection, x402
//! metering, dispute windows, or any mainnet safety. Do not point it at mainnet.
//!
//! ## What moves, and when
//!
//! - `escrow` — delegates to the inner [`scemadex_sdk::EscrowBondEngine`]: sizes a
//!   conviction-weighted bond and a guaranteed-minimum output. No transfer.
//! - `settle` — runs the honored/slashed decision. On **`Slashed`**, transfers
//!   `bond.amount` micro-USDC from the agent's USDC account to the caller's. On
//!   **`Honored`**, nothing moves (the agent keeps its collateral).
//!
//! ```no_run
//! use std::sync::Arc;
//! use scemadex_settle::DevnetUsdcSettler;
//! use solana_sdk::{pubkey::Pubkey, signature::Keypair};
//! # use std::str::FromStr;
//! # async fn run() -> anyhow::Result<()> {
//! let agent = Arc::new(Keypair::new());            // funded with devnet USDC + SOL
//! let usdc_mint = Pubkey::from_str("...")?;        // your devnet SPL mint
//! let beneficiary = Pubkey::from_str("...")?;      // caller's USDC token account
//! let settler = DevnetUsdcSettler::devnet(agent, usdc_mint, beneficiary);
//! # let _ = settler;
//! # Ok(()) }
//! ```

pub mod optimistic;
pub use optimistic::{Beneficiaries, OptimisticUsdcSettler, SlashTransfer};

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature, Signer};
use solana_sdk::transaction::Transaction;
use spl_associated_token_account::get_associated_token_address;

use scemadex_sdk::{
    Bond, BondConfig, BondEngine, BondLedger, BondOutcome, Conviction, EscrowBondEngine, Fill,
    Result, ScemaDexError, Solution, Usdc,
};

/// Public Solana devnet RPC endpoint.
pub const DEVNET_RPC: &str = "https://api.devnet.solana.com";

/// USDC has 6 decimals on Solana; bonds are denominated in micro-USDC.
pub const USDC_DECIMALS: u8 = 6;

/// A devnet settler: the [`EscrowBondEngine`] state machine plus a real SPL-USDC
/// transfer on slash. See the crate docs for the devnet-only caveat.
pub struct DevnetUsdcSettler {
    inner: EscrowBondEngine,
    rpc: RpcClient,
    agent: Arc<Keypair>,
    usdc_mint: Pubkey,
    /// The caller's USDC token account — receives the bond when it is slashed.
    beneficiary_token_account: Pubkey,
    last_signature: Mutex<Option<Signature>>,
}

impl DevnetUsdcSettler {
    /// Construct against an explicit RPC endpoint and bond configuration.
    pub fn new(
        rpc_url: impl Into<String>,
        agent: Arc<Keypair>,
        usdc_mint: Pubkey,
        beneficiary_token_account: Pubkey,
        config: BondConfig,
    ) -> Self {
        Self {
            inner: EscrowBondEngine::new(config),
            rpc: RpcClient::new_with_commitment(rpc_url.into(), CommitmentConfig::confirmed()),
            agent,
            usdc_mint,
            beneficiary_token_account,
            last_signature: Mutex::new(None),
        }
    }

    /// Convenience: the public devnet RPC with the default [`BondConfig`].
    pub fn devnet(
        agent: Arc<Keypair>,
        usdc_mint: Pubkey,
        beneficiary_token_account: Pubkey,
    ) -> Self {
        Self::new(
            DEVNET_RPC,
            agent,
            usdc_mint,
            beneficiary_token_account,
            BondConfig::default(),
        )
    }

    /// The agent's USDC associated-token account (the bond's funding source).
    pub fn agent_usdc_account(&self) -> Pubkey {
        get_associated_token_address(&self.agent.pubkey(), &self.usdc_mint)
    }

    /// The inference fee for a given conviction (delegates to the inner engine).
    pub fn quote_fee(&self, conviction: Conviction) -> Usdc {
        self.inner.quote_fee(conviction)
    }

    /// Snapshot of the honored/slashed ledger.
    pub fn ledger(&self) -> BondLedger {
        self.inner.ledger()
    }

    /// Number of bonds currently escrowed (awaiting settlement).
    pub fn open_bonds(&self) -> usize {
        self.inner.open_bonds()
    }

    /// The signature of the most recent on-chain slash transfer, if any.
    pub fn last_signature(&self) -> Option<Signature> {
        self.last_signature.lock().ok().and_then(|s| *s)
    }

    /// Like [`BondEngine::settle`] but also returns the on-chain transfer
    /// signature when the bond was slashed (`None` when honored).
    pub async fn settle_onchain(
        &self,
        bond: &Bond,
        fill: &Fill,
    ) -> Result<(BondOutcome, Option<Signature>)> {
        let outcome = self.inner.settle(bond, fill).await?;
        let sig = match outcome {
            BondOutcome::Slashed => Some(self.transfer_bond(bond.amount).await?),
            BondOutcome::Honored => None,
        };
        if let Some(sig) = sig {
            if let Ok(mut slot) = self.last_signature.lock() {
                *slot = Some(sig);
            }
            tracing::info!(
                amount_micro_usdc = bond.amount.0,
                signature = %sig,
                "bond slashed — devnet USDC transferred to caller"
            );
        }
        Ok((outcome, sig))
    }

    /// Transfer `amount` micro-USDC from the agent's USDC account to the
    /// beneficiary on devnet, signed by the agent.
    async fn transfer_bond(&self, amount: Usdc) -> Result<Signature> {
        let source = self.agent_usdc_account();
        let ix = spl_token::instruction::transfer_checked(
            &spl_token::id(),
            &source,
            &self.usdc_mint,
            &self.beneficiary_token_account,
            &self.agent.pubkey(),
            &[],
            amount.0,
            USDC_DECIMALS,
        )
        .map_err(|e| ScemaDexError::Bond(format!("build transfer ix: {e}")))?;

        let blockhash = self
            .rpc
            .get_latest_blockhash()
            .await
            .map_err(|e| ScemaDexError::Bond(format!("get blockhash: {e}")))?;
        let tx = Transaction::new_signed_with_payer(
            &[ix],
            Some(&self.agent.pubkey()),
            &[self.agent.as_ref()],
            blockhash,
        );
        self.rpc
            .send_and_confirm_transaction(&tx)
            .await
            .map_err(|e| ScemaDexError::Bond(format!("submit slash transfer: {e}")))
    }
}

#[async_trait]
impl BondEngine for DevnetUsdcSettler {
    async fn escrow(&self, solution: &Solution) -> Result<Bond> {
        self.inner.escrow(solution).await
    }

    /// Settles the bond and, on slash, performs the devnet USDC transfer. Use
    /// [`DevnetUsdcSettler::settle_onchain`] if you need the transfer signature.
    async fn settle(&self, bond: &Bond, fill: &Fill) -> Result<BondOutcome> {
        self.settle_onchain(bond, fill).await.map(|(o, _)| o)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn settler() -> DevnetUsdcSettler {
        DevnetUsdcSettler::devnet(
            Arc::new(Keypair::new()),
            Pubkey::from_str("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU").unwrap(),
            Pubkey::new_unique(),
        )
    }

    #[test]
    fn agent_usdc_account_is_deterministic() {
        let s = settler();
        assert_eq!(s.agent_usdc_account(), s.agent_usdc_account());
    }

    #[tokio::test]
    async fn honored_settlement_moves_nothing_offline() {
        use scemadex_sdk::RoutePolicy;
        // A fill that meets the guarantee settles Honored with no RPC call.
        let s = settler();
        let sol = scemadex_sdk::ReferenceRoutePolicy
            .solve(&scemadex_sdk::demo_intent())
            .await
            .unwrap();
        let bond = s.escrow(&sol).await.unwrap();
        let fill = Fill {
            amount_out: scemadex_sdk::Amount::new(bond.min_out_raw, USDC_DECIMALS),
            executed_unix: 0,
        };
        let (outcome, sig) = s.settle_onchain(&bond, &fill).await.unwrap();
        assert_eq!(outcome, BondOutcome::Honored);
        assert!(sig.is_none(), "honored settlement must not touch the chain");
    }
}
