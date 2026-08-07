use crate::SwapInstructionBuilder;
use anyhow::Result;
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose};
use scematica_core::types::DexKind;
use solana_sdk::{instruction::Instruction, pubkey::Pubkey, transaction::VersionedTransaction};

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

        let response = self
            .http_client
            .post(format!("{}/swap", self.api_url))
            .json(&payload)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!(
                "Jupiter /swap returned HTTP {}: {}",
                status.as_u16(),
                body
            );
        }

        let resp: serde_json::Value = response.json().await?;

        let tx_b64 = resp["swapTransaction"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("No swapTransaction in Jupiter response"))?;

        Ok(general_purpose::STANDARD.decode(tx_b64)?)
    }

    /// Deserialize a bincode-encoded `VersionedTransaction` from raw bytes.
    ///
    /// Returns `Err` on empty input, malformed bytes, or any bincode decode failure (Req 8.3).
    pub fn deserialize_transaction(&self, tx_bytes: &[u8]) -> Result<VersionedTransaction> {
        bincode::deserialize::<VersionedTransaction>(tx_bytes).map_err(Into::into)
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
        _owner: &Pubkey,
        _token_in: &Pubkey,
        _token_out: &Pubkey,
        _ata_in: &Pubkey,
        _ata_out: &Pubkey,
        _amount_in: u64,
        _min_amount_out: u64,
    ) -> Result<Vec<Instruction>> {
        // Jupiter returns a full versioned transaction, not individual instructions.
        // For arb use, we prefer direct DEX instructions to avoid Jupiter's overhead.
        // This builder is provided for single-hop swaps via the sniper.
        // Returns empty — caller should use get_swap_transaction() directly.
        tracing::warn!("JupiterBuilder::build_swap called — use get_swap_transaction() for Jupiter swaps");
        Ok(vec![])
    }
}
