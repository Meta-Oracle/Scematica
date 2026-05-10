use crate::SwapInstructionBuilder;
use anyhow::Result;
use async_trait::async_trait;
use scematica_core::types::DexKind;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey};

/// Jupiter V6 aggregator swap builder
/// Uses Jupiter's REST API to get the optimal route and swap transaction
pub struct JupiterBuilder {
    http_client: reqwest::Client,
    api_url: String,
}

impl JupiterBuilder {
    pub fn new() -> Self {
        Self {
            http_client: reqwest::Client::new(),
            api_url: "https://quote-api.jup.ag/v6".into(),
        }
    }

    /// Get a swap quote from Jupiter API
    pub async fn get_quote(
        &self,
        input_mint: &Pubkey,
        output_mint: &Pubkey,
        amount: u64,
        slippage_bps: u16,
    ) -> Result<serde_json::Value> {
        let url = format!(
            "{}/quote?inputMint={}&outputMint={}&amount={}&slippageBps={}",
            self.api_url, input_mint, output_mint, amount, slippage_bps
        );
        let resp = self.http_client.get(&url).send().await?.json().await?;
        Ok(resp)
    }

    /// Get a swap transaction from Jupiter API
    pub async fn get_swap_transaction(
        &self,
        quote: &serde_json::Value,
        user_public_key: &Pubkey,
    ) -> Result<Vec<u8>> {
        let payload = serde_json::json!({
            "quoteResponse": quote,
            "userPublicKey": user_public_key.to_string(),
            "wrapAndUnwrapSol": true,
            "dynamicComputeUnitLimit": true,
            "prioritizationFeeLamports": "auto"
        });

        let resp: serde_json::Value = self
            .http_client
            .post(format!("{}/swap", self.api_url))
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;

        let tx_b64 = resp["swapTransaction"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No swapTransaction in Jupiter response"))?;

        Ok(base64::decode(tx_b64)?)
    }
}

#[async_trait]
impl SwapInstructionBuilder for JupiterBuilder {
    fn dex(&self) -> DexKind {
        DexKind::Jupiter
    }

    async fn build_swap(
        &self,
        _pool: &Pubkey,
        owner: &Pubkey,
        token_in: &Pubkey,
        token_out: &Pubkey,
        _ata_in: &Pubkey,
        _ata_out: &Pubkey,
        amount_in: u64,
        min_amount_out: u64,
    ) -> Result<Vec<Instruction>> {
        // Jupiter returns a full versioned transaction, not individual instructions.
        // For arb use, we prefer direct DEX instructions to avoid Jupiter's overhead.
        // This builder is provided for single-hop swaps via the sniper.
        // Returns empty — caller should use get_swap_transaction() directly.
        tracing::warn!("JupiterBuilder::build_swap called — use get_swap_transaction() for Jupiter swaps");
        Ok(vec![])
    }
}
