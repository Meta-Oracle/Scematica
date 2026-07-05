//! **Optimistic settler** — the dispute-window state machine wired to real USDC.
//!
//! [`crate::DevnetUsdcSettler`] closes the loop *atomically*: a slash pays the caller
//! in one transfer, with no window and no split. This module promotes that to the
//! full **Settlement v2** shape: it drives [`scemadex_sdk::SettlementMachine`] with a
//! dispute window and, at **Finalized(Slashed)**, disburses the bond across the
//! four-way [`scemadex_sdk::SlashRouting`] (caller / challengers / insurance /
//! lineage) as real on-chain SPL-USDC transfers. Honored bonds move nothing.
//!
//! It is the facilitator-side *driver* of the money loop. The **trustless custody**
//! upgrade — where the program, not the agent, holds the collateral until finality —
//! is the `scemadex-escrow` Anchor program (`programs/scemadex-escrow`); this settler
//! and that program are the two halves of "close the loop on mainnet." Point the
//! settler at devnet to exercise the whole optimistic lifecycle for free.
//!
//! Generic over a [`Clock`] so the window/deadline logic is deterministic in tests
//! (inject a [`scemadex_sdk::ManualClock`]); defaults to the wall clock.

use std::sync::{Arc, Mutex};

use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature, Signer};
use solana_sdk::transaction::Transaction;
use spl_associated_token_account::get_associated_token_address;

use scemadex_sdk::{
    Bond, BondOutcome, Clock, Result, ScemaDexError, SettlementConfig, SettlementMachine,
    SlashDistribution, SystemClock, Usdc,
};

use crate::{DEVNET_RPC, USDC_DECIMALS};

/// The four token accounts a slashed bond is split across, per [`scemadex_sdk::SlashRouting`].
/// Any share whose routed bps is zero simply receives nothing.
#[derive(Clone, Copy, Debug)]
pub struct Beneficiaries {
    /// The wronged caller (absorbs routing dust).
    pub caller: Pubkey,
    /// Counter-Market winners' payout account (Primitive E).
    pub challengers: Pubkey,
    /// Reinsurance pool token account (Primitive J).
    pub insurance: Pubkey,
    /// Upstream experience-royalty account (Primitive G).
    pub lineage: Pubkey,
}

/// One on-chain transfer performed while settling a slash.
#[derive(Clone, Copy, Debug)]
pub struct SlashTransfer {
    pub destination: Pubkey,
    pub amount: Usdc,
    pub signature: Signature,
}

/// An optimistic USDC settler: a dispute-windowed [`SettlementMachine`] plus real
/// four-way slash disbursement on finality.
pub struct OptimisticUsdcSettler<C: Clock = SystemClock> {
    machine: SettlementMachine<C>,
    rpc: RpcClient,
    agent: Arc<Keypair>,
    usdc_mint: Pubkey,
    beneficiaries: Beneficiaries,
    last_transfers: Mutex<Vec<SlashTransfer>>,
}

impl OptimisticUsdcSettler<SystemClock> {
    /// A wall-clock settler against the public devnet RPC with an optimistic window.
    pub fn devnet(
        agent: Arc<Keypair>,
        usdc_mint: Pubkey,
        beneficiaries: Beneficiaries,
        config: SettlementConfig,
    ) -> Self {
        Self::new(DEVNET_RPC, agent, usdc_mint, beneficiaries, config, SystemClock)
    }
}

impl<C: Clock> OptimisticUsdcSettler<C> {
    /// Construct against an explicit RPC endpoint, settlement config, and clock.
    pub fn new(
        rpc_url: impl Into<String>,
        agent: Arc<Keypair>,
        usdc_mint: Pubkey,
        beneficiaries: Beneficiaries,
        config: SettlementConfig,
        clock: C,
    ) -> Self {
        Self {
            machine: SettlementMachine::new(config, clock),
            rpc: RpcClient::new_with_commitment(rpc_url.into(), CommitmentConfig::confirmed()),
            agent,
            usdc_mint,
            beneficiaries,
            last_transfers: Mutex::new(Vec::new()),
        }
    }

    /// The underlying state machine — register bonds, provision fills, drive the
    /// clock, inspect slash distributions.
    pub fn machine(&self) -> &SettlementMachine<C> {
        &self.machine
    }

    /// The agent's USDC associated-token account (the disbursement source).
    pub fn agent_usdc_account(&self) -> Pubkey {
        get_associated_token_address(&self.agent.pubkey(), &self.usdc_mint)
    }

    /// The on-chain transfers performed by the most recent slash settlement.
    pub fn last_transfers(&self) -> Vec<SlashTransfer> {
        self.last_transfers.lock().map(|t| t.clone()).unwrap_or_default()
    }

    /// Register a freshly escrowed bond as `Escrowed`.
    pub fn open(&self, bond: &Bond) -> Result<()> {
        self.machine.open(bond)
    }

    /// Record a fill's provisional outcome and open the dispute window.
    pub fn provision(&self, digest: &str, outcome: BondOutcome) -> Result<()> {
        self.machine.provision(digest, outcome).map(|_| ())
    }

    /// Finalize a single matured bond and, on **Slashed**, disburse the four-way
    /// split on-chain. Returns the outcome and every transfer performed (empty on an
    /// honor — the agent keeps its collateral, nothing moves).
    pub async fn finalize_and_settle(&self, digest: &str) -> Result<(BondOutcome, Vec<SlashTransfer>)> {
        let outcome = self.machine.finalize(digest)?;
        self.disburse(digest, outcome).await
    }

    /// Finalize every matured, undisputed bond and settle each on-chain. Disputed
    /// bonds are left for an explicit resolution path. Returns `(digest, outcome)`
    /// per settled bond.
    pub async fn sweep_and_settle(&self) -> Result<Vec<(String, BondOutcome)>> {
        let matured = self.machine.sweep()?;
        let mut out = Vec::with_capacity(matured.len());
        for (digest, outcome) in matured {
            self.disburse(&digest, outcome).await?;
            out.push((digest, outcome));
        }
        Ok(out)
    }

    /// Disburse a just-finalized bond. Honored → nothing; Slashed → the four-way
    /// split, one transfer per non-zero share.
    async fn disburse(&self, digest: &str, outcome: BondOutcome) -> Result<(BondOutcome, Vec<SlashTransfer>)> {
        let transfers = match outcome {
            BondOutcome::Honored => Vec::new(),
            BondOutcome::Slashed => {
                let dist = self
                    .machine
                    .slash_distribution(digest)
                    .ok_or_else(|| ScemaDexError::Bond(format!("no slash distribution for {digest}")))?;
                self.settle_split(dist).await?
            }
        };
        if let Ok(mut slot) = self.last_transfers.lock() {
            slot.clone_from(&transfers);
        }
        Ok((outcome, transfers))
    }

    /// Execute one on-chain transfer per non-zero routed share.
    async fn settle_split(&self, dist: SlashDistribution) -> Result<Vec<SlashTransfer>> {
        let legs = [
            (self.beneficiaries.caller, dist.caller),
            (self.beneficiaries.challengers, dist.challengers),
            (self.beneficiaries.insurance, dist.insurance),
            (self.beneficiaries.lineage, dist.lineage),
        ];
        let mut transfers = Vec::new();
        for (destination, amount) in legs {
            if amount.0 == 0 {
                continue;
            }
            let signature = self.transfer(destination, amount).await?;
            transfers.push(SlashTransfer { destination, amount, signature });
            tracing::info!(
                %destination,
                amount_micro_usdc = amount.0,
                %signature,
                "slash slice transferred"
            );
        }
        Ok(transfers)
    }

    /// Transfer `amount` micro-USDC from the agent's USDC account to `destination`,
    /// signed by the agent.
    async fn transfer(&self, destination: Pubkey, amount: Usdc) -> Result<Signature> {
        let source = self.agent_usdc_account();
        let ix = spl_token::instruction::transfer_checked(
            &spl_token::id(),
            &source,
            &self.usdc_mint,
            &destination,
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

#[cfg(test)]
mod tests {
    use super::*;
    use scemadex_sdk::{ManualClock, SlashRouting};
    use std::str::FromStr;

    fn beneficiaries() -> Beneficiaries {
        Beneficiaries {
            caller: Pubkey::new_unique(),
            challengers: Pubkey::new_unique(),
            insurance: Pubkey::new_unique(),
            lineage: Pubkey::new_unique(),
        }
    }

    fn settler(config: SettlementConfig) -> OptimisticUsdcSettler<ManualClock> {
        OptimisticUsdcSettler::new(
            DEVNET_RPC,
            Arc::new(Keypair::new()),
            Pubkey::from_str("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU").unwrap(),
            beneficiaries(),
            config,
            ManualClock::new(1_000),
        )
    }

    fn bond(digest: &str, amount: u64) -> Bond {
        Bond {
            intent_digest: digest.into(),
            amount: Usdc(amount),
            min_out_raw: 1_000_000,
            deadline_unix: 0,
        }
    }

    #[tokio::test]
    async fn honored_bond_moves_nothing_offline() {
        // A provisional honor that elapses its window settles with zero transfers —
        // no RPC is touched, so this runs fully offline.
        let s = settler(SettlementConfig::optimistic(60));
        s.open(&bond("h", 1_000)).unwrap();
        s.provision("h", BondOutcome::Honored).unwrap();
        s.machine().clock().advance(61);
        let (outcome, transfers) = s.finalize_and_settle("h").await.unwrap();
        assert_eq!(outcome, BondOutcome::Honored);
        assert!(transfers.is_empty(), "an honored bond disburses nothing");
    }

    #[test]
    fn window_gates_finalization() {
        // While the window is open, there is nothing to settle yet.
        let s = settler(SettlementConfig::optimistic(60));
        s.open(&bond("d", 1_000)).unwrap();
        s.provision("d", BondOutcome::Honored).unwrap();
        assert!(s.machine().sweep().unwrap().is_empty(), "window still open");
    }

    #[test]
    fn slash_distribution_uses_the_configured_routing() {
        // The split the settler would disburse is exactly the machine's four-way
        // distribution — assert the routing math offline before any transfer.
        let routing = SlashRouting {
            to_caller_bps: 4_000,
            to_challengers_bps: 3_000,
            to_insurance_bps: 2_000,
            to_lineage_bps: 1_000,
        };
        let s = settler(SettlementConfig::optimistic(0).with_slash_routing(routing));
        s.open(&bond("s", 1_003)).unwrap();
        // Zero window → provision finalizes immediately as Slashed.
        s.provision("s", BondOutcome::Slashed).unwrap();
        let d = s.machine().slash_distribution("s").unwrap();
        assert_eq!(d.caller.0 + d.challengers.0 + d.insurance.0 + d.lineage.0, 1_003);
        assert_eq!(d.challengers, Usdc(300));
        assert_eq!(d.insurance, Usdc(200));
        assert_eq!(d.lineage, Usdc(100));
        assert_eq!(d.caller, Usdc(403), "caller absorbs the dust");
    }
}
