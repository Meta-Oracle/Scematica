# Implementation Plan: Live Trading Execution

## Overview

Fix the four broken DEX execution paths by injecting a real `Arc<RpcClient>` into each builder, fetching on-chain pool state at swap time, and routing Jupiter pools through the versioned-transaction path. The work touches `scematica-executor` (all four builders + factory), `scematica-sniper` (Sniper + SellMonitor), and `scematica-arb` (ArbExecutor, which also calls `get_builder`).

## Tasks

- [x] 1. Update `get_builder` factory to accept `Arc<RpcClient>`
  - Modify `scematica-executor/src/lib.rs`: change `get_builder(dex: DexKind)` signature to `get_builder(dex: DexKind, rpc: Arc<RpcClient>) -> Option<Box<dyn SwapInstructionBuilder>>`
  - Add `use solana_client::nonblocking::rpc_client::RpcClient` and `use std::sync::Arc` imports to `lib.rs`
  - Pass `rpc.clone()` to `RaydiumBuilder::new`, `OrcaBuilder::new`, and `MeteoraBuilder::new`; pass nothing extra to `JupiterBuilder::new` (Jupiter uses HTTP)
  - _Requirements: 1.4, 10.1, 10.2, 10.3, 10.4, 10.5_

- [ ] 2. Implement `RaydiumBuilder` with real on-chain state fetching
  - [x] 2.1 Add `Arc<RpcClient>` field to `RaydiumBuilder` and update `new`
    - Change `pub struct RaydiumBuilder` to hold `rpc: Arc<RpcClient>`
    - Update `RaydiumBuilder::new(rpc: Arc<RpcClient>) -> Self`
    - No RPC calls at construction time
    - _Requirements: 1.1, 1.2, 1.3_

  - [x] 2.2 Implement pool state fetch and Borsh decode in `build_swap`
    - Call `self.rpc.get_account_data(pool).await?`; return `Err` if RPC fails (Req 2.4) or `data.len() < 752` (Req 2.3)
    - Deserialize with `RaydiumAmmV4::try_from_slice(&data[..752])?`
    - Return `Err` if `state.status == 0` (Req 2.5)
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

  - [-] 2.3 Implement Serum market fetch and account extraction in `build_swap`
    - Call `self.rpc.get_account_data(state.market_id).await?`; return `Err` if RPC fails (Req 2.7) or `data.len() < 388` (Req 2.8)
    - Extract `vault_signer_nonce` from `market_data[37..45]`, `event_queue` from `[245..277]`, `bids` from `[277..309]`, `asks` from `[309..341]`, `coin_vault` from `[109..141]`, `pc_vault` from `[157..189]` (all offsets are `5 + base_offset`)
    - Derive vault signer via `Pubkey::create_program_address(&[market_id.as_ref(), &nonce.to_le_bytes()], &state.market_program_id)?` (Req 2.10, 2.11)
    - _Requirements: 2.6, 2.7, 2.8, 2.9, 2.10, 2.11_

  - [~] 2.4 Populate all 18 account metas and return the instruction
    - Derive AMM authority PDA: `Pubkey::find_program_address(&[b"amm authority"], &RAYDIUM_AMM_V4).0`
    - Build the 18-account `Vec<AccountMeta>` in the order specified in the design (accounts 0–17)
    - Ensure no account is `Pubkey::default()` before returning
    - Return `vec![Instruction { program_id: RAYDIUM_AMM_V4, accounts, data: raydium_swap_data(amount_in, min_amount_out) }]`
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6, 3.7, 3.8, 3.9, 3.10, 3.11, 3.12, 3.13, 3.14, 3.15, 3.16_

  - [~] 2.5 Write property test for `raydium_swap_data` encoding round-trip
    - **Property 1: Raydium swap data encoding round-trip**
    - For any `amount_in: u64` and `min_amount_out: u64`, assert output is 17 bytes, `data[0] == 9u8`, `u64::from_le_bytes(data[1..9]) == amount_in`, `u64::from_le_bytes(data[9..17]) == min_amount_out`
    - Use `proptest` in `scematica-executor/src/raydium.rs` under `#[cfg(test)]`
    - **Validates: Requirements 3.4, 3.5, 3.6, 11.1**

  - [~] 2.6 Write property test for Raydium pool data length guard
    - **Property 3: Raydium pool data length guard**
    - For any byte slice with `len < 752`, assert `build_swap` returns `Err` (use a mock/stub that returns short data)
    - **Validates: Requirements 2.3**

- [ ] 3. Implement `OrcaBuilder` with Whirlpool state fetching and tick array PDA derivation
  - [x] 3.1 Add `Arc<RpcClient>` field to `OrcaBuilder` and update `new`
    - Change `pub struct OrcaBuilder` to hold `rpc: Arc<RpcClient>`
    - Update `OrcaBuilder::new(rpc: Arc<RpcClient>) -> Self`
    - _Requirements: 4.1_

  - [x] 3.2 Implement Whirlpool state fetch and field extraction in `build_swap`
    - Call `self.rpc.get_account_data(pool).await?`; return `Err` if RPC fails or `data.len() < 272` (Req 4.3)
    - Skip 8-byte Anchor discriminator; extract `tick_spacing` at `8+9` (u16), `tick_current_index` at `8+37` (i32), `token_mint_a` at `8+101`, `token_vault_a` at `8+133`, `token_mint_b` at `8+181`, `token_vault_b` at `8+213`
    - Set `a_to_b = (token_in == token_mint_a)`; return `Err` if `token_in` matches neither mint (Req 4.6)
    - Set `sqrt_price_limit` to `MIN_SQRT_PRICE` (4295048016u128) when `a_to_b`, else `MAX_SQRT_PRICE` (79226673515401279992447579055u128)
    - _Requirements: 4.2, 4.3, 4.4, 4.5, 4.6, 4.7, 4.8_

  - [~] 3.3 Implement tick array PDA derivation and oracle PDA derivation
    - Extract `derive_tick_array_pda` helper: `Pubkey::find_program_address(&[b"tick_array", pool.as_ref(), start_tick.to_string().as_bytes()], &ORCA_WHIRLPOOL).0`
    - Compute `start_tick_index(tick_current_index, tick_spacing, offset)` using `div_euclid` as specified in design
    - Derive 3 tick arrays at offsets `0`, `±1`, `±2` based on `a_to_b` direction
    - Derive oracle PDA: `Pubkey::find_program_address(&[b"oracle", pool.as_ref()], &ORCA_WHIRLPOOL).0`
    - _Requirements: 4.9, 4.10_

  - [~] 3.4 Populate all 11 account metas and return the instruction
    - Assign `(user_token_a, user_token_b)` based on `a_to_b` direction (Req 4.11)
    - Build the 11-account `Vec<AccountMeta>` in the order: `[spl_token, owner, pool, user_token_a, token_vault_a, user_token_b, token_vault_b, tick_array_0, tick_array_1, tick_array_2, oracle]` (Req 4.12)
    - Encode instruction data: `WHIRLPOOL_SWAP_DISCRIMINATOR ++ amount_in ++ min_amount_out ++ sqrt_price_limit ++ 1u8 ++ a_to_b as u8` (42 bytes total)
    - _Requirements: 5.1, 5.2, 5.3, 5.4, 5.5, 5.6, 5.7, 5.8, 5.9, 5.10, 11.2_

  - [~] 3.5 Write property test for Orca swap direction correctness
    - **Property 5: Orca swap direction is always correct**
    - For any two distinct random `Pubkey` values used as `token_mint_a` and `token_mint_b`, assert `a_to_b == (token_in == token_mint_a)` in the built instruction's last byte
    - **Validates: Requirements 4.5**

  - [~] 3.6 Write property test for tick array PDA distinctness and determinism
    - **Property 7: Orca tick array PDAs are distinct and deterministic**
    - For any `tick_current_index` in `[-443636, 443636]` and `tick_spacing` in `[1, 128]`, assert the 3 derived PDAs are pairwise distinct and each is a valid `ORCA_WHIRLPOOL` PDA
    - **Validates: Requirements 4.10**

- [ ] 4. Implement `MeteoraBuilder` with DLMM state fetching and bin array PDA derivation
  - [x] 4.1 Add `Arc<RpcClient>` field to `MeteoraBuilder` and update `new`
    - Change `pub struct MeteoraBuilder` to hold `rpc: Arc<RpcClient>`
    - Update `MeteoraBuilder::new(rpc: Arc<RpcClient>) -> Self`
    - _Requirements: 6.1_

  - [x] 4.2 Implement DLMM LbPair state fetch and field extraction in `build_swap`
    - Call `self.rpc.get_account_data(pool).await?`; return `Err` if RPC fails or data is too short (Req 6.3)
    - Skip 8-byte Anchor discriminator; extract `reserve_x`, `reserve_y`, `token_x_mint`, `token_y_mint`, `active_id` (i32), `bin_step` (u16), `oracle` (Pubkey), and `bin_array_bitmap_extension` (Pubkey) from the Anchor-serialized layout
    - Set `swap_for_y = (token_in == token_x_mint)`; return `Err` if `token_in` matches neither mint (Req 6.6)
    - _Requirements: 6.2, 6.3, 6.4, 6.5, 6.6_

  - [~] 4.3 Implement bin array PDA derivation
    - Compute `bin_array_index = active_id.div_euclid(70)`
    - Derive 2 bin array PDAs: `Pubkey::find_program_address(&[b"bin_array", pool.as_ref(), &(index as i64).to_le_bytes()], &METEORA_DLMM).0` for `index` and `index + 1`
    - _Requirements: 6.7_

  - [~] 4.4 Populate all 14 account metas and return the instruction
    - Build the 14-account `Vec<AccountMeta>` in the order: `[pool, bin_array_bitmap_extension, reserve_x, reserve_y, ata_in, ata_out, token_x_mint, token_y_mint, oracle, bin_array_lower, bin_array_upper, owner, spl_token, spl_token]`
    - Encode instruction data: `METEORA_SWAP_DISCRIMINATOR ++ amount_in ++ min_amount_out ++ swap_for_y as u8` (25 bytes total)
    - Note: fix the existing discriminator constant — design specifies `[0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x75, 0x27, 0x43]` (current code has `0x44` as last byte, which is wrong)
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 7.6, 7.7, 7.8, 7.9, 7.10, 11.3_

  - [~] 4.5 Write property test for Meteora swap direction correctness
    - **Property 8: Meteora swap direction is always correct**
    - For any two distinct random `Pubkey` values used as `token_x_mint` and `token_y_mint`, assert `swap_for_y == (token_in == token_x_mint)` in the built instruction data byte at index 24
    - **Validates: Requirements 6.5**

- [~] 5. Checkpoint — ensure all builder unit tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 6. Add `JupiterBuilder::deserialize_transaction` method
  - [x] 6.1 Implement `deserialize_transaction` on `JupiterBuilder`
    - Add `pub fn deserialize_transaction(&self, tx_bytes: &[u8]) -> Result<VersionedTransaction>`
    - Implement as `bincode::deserialize::<VersionedTransaction>(tx_bytes).map_err(Into::into)`
    - Return `Err` on empty or malformed bytes (Req 8.3)
    - Add `use solana_sdk::transaction::VersionedTransaction` import
    - _Requirements: 8.1, 8.2, 8.3_

  - [x] 6.2 Harden `get_swap_transaction` error handling
    - Ensure non-200 HTTP responses return `Err` containing the status code (Req 8.5)
    - Ensure missing `swapTransaction` field returns `Err("No swapTransaction in Jupiter response")` — this already exists but verify it is reachable on non-200 paths
    - Keep `build_swap` returning empty `Vec` with `tracing::warn!` (Req 8.6)
    - _Requirements: 8.4, 8.5, 8.6_

  - [~] 6.3 Write property test for `deserialize_transaction` round-trip
    - **Property 10: Jupiter VersionedTransaction deserialization round-trip**
    - Construct a `VersionedTransaction` with a known message, serialize with `bincode`, call `deserialize_transaction`, assert message content and signature count are identical
    - **Validates: Requirements 8.2**

- [ ] 7. Update `Sniper` to use Jupiter versioned transaction path and fix buy slippage
  - [~] 7.1 Add `jupiter_builder` field to `Sniper` and `SellMonitor`; pass `Arc<RpcClient>` to `get_builder`
    - Add `jupiter_builder: Arc<JupiterBuilder>` field to `Sniper` and `SellMonitor`
    - In `Sniper::new`, change `get_builder(DexKind::Raydium)` to `get_builder(DexKind::Raydium, rpc.clone())`
    - Construct `jupiter_builder: Arc::new(JupiterBuilder::new())`
    - Update `clone_for_sell` to include `jupiter_builder`
    - _Requirements: 9.1, 10.6_

  - [~] 7.2 Implement Jupiter swap path in `Sniper::buy`
    - When `pool.dex == DexKind::Jupiter`, call `self.jupiter_builder.get_quote(token_in, token_out, amount_in, slippage_bps).await?`
    - Call `self.jupiter_builder.get_swap_transaction(&quote, &wallet_pubkey).await?`
    - Call `self.jupiter_builder.deserialize_transaction(&tx_bytes)?` to get `VersionedTransaction`
    - Fetch fresh blockhash via `self.rpc.get_latest_blockhash().await?`, re-sign with wallet keypair, submit via `self.rpc.send_and_confirm_transaction(&versioned_tx).await?`
    - Wrap the entire Jupiter path in the existing retry loop; log errors at `error!` level on each failure (Req 9.5)
    - _Requirements: 9.1, 9.2, 9.3, 9.4, 9.5_

  - [~] 7.3 Fix buy slippage: compute real `min_amount_out` in `Sniper::buy`
    - Fetch `quote_reserve` and `base_reserve` from `pool.quote_vault` and `pool.base_vault` via `rpc.get_token_account_balance`
    - Compute `estimated_out = (self.quote_amount_raw as u128 * quote_reserve as u128 / base_reserve as u128) as u64`
    - Compute `min_amount_out = apply_slippage(estimated_out, self.config.buy_slippage_pct)`
    - Pass `min_amount_out` (not `0`) to `build_swap` and to the Jupiter quote `slippage_bps`
    - _Requirements: 13.1, 13.2_

  - [~] 7.4 Fix sell slippage in `Sniper::sell` and `SellMonitor::do_sell`
    - Replace the hardcoded `(amount as f64 * 0.99) as u64` estimate with the same reserve-based formula used in buy
    - Pass the result through `apply_slippage(estimated_out, self.config.sell_slippage_pct)`
    - _Requirements: 13.3_

  - [~] 7.5 Fix retry logic: treat empty `Vec<Instruction>` as a failed attempt
    - After `build_swap` returns `Ok(ixs)`, check `if ixs.is_empty()` and treat it as a failure (increment retry counter, do not submit)
    - Log at `warn!` level when an empty instruction vec is returned
    - _Requirements: 12.4_

  - [~] 7.6 Write property test for slippage application
    - **Property 11: Slippage is always applied to swap min_amount_out**
    - For any `estimated_out: u64` and `slippage_pct: f64` in `[0.0, 100.0)`, assert `apply_slippage(estimated_out, slippage_pct) < estimated_out` when `slippage_pct > 0`, and equals `estimated_out` when `slippage_pct == 0`
    - Place in `scematica-core/src/token.rs` under `#[cfg(test)]`
    - **Validates: Requirements 13.1, 13.3**

- [x] 8. Update `ArbExecutor` callers of `get_builder` (breaking change propagation)
  - [x] 8.1 Update `scematica-arb/src/executor.rs` to pass `Arc<RpcClient>` to `get_builder`
    - `ArbExecutor` already holds `rpc: Arc<RpcConnection>`; extract the inner `Arc<RpcClient>` via `self.rpc.client.clone()` (or the appropriate field name)
    - Change each `get_builder(dex)` call to `get_builder(dex, rpc.clone())`
    - _Requirements: 10.1_

- [ ] 9. Add `proptest` dev-dependency and wire up all property tests
  - [x] 9.1 Add `proptest` to `[dev-dependencies]` in `scematica-executor/Cargo.toml` and `scematica-core/Cargo.toml`
    - Add `proptest = "1"` to both crates' `[dev-dependencies]`
    - Verify `proptest` is not already present to avoid duplicate entries
    - _Requirements: (testing infrastructure)_

  - [ ] 9.2 Write property test for `RaydiumAmmV4` Borsh round-trip
    - **Property 4: RaydiumAmmV4 state deserialization round-trip**
    - For any `RaydiumAmmV4` struct with arbitrary field values, serialize with Borsh and deserialize; assert all fields are identical
    - Place in `scematica-executor/src/raydium_state.rs` under `#[cfg(test)]`
    - **Validates: Requirements 2.2**

  - [~] 9.3 Write property test for retry count bound
    - **Property 12: Retry count never exceeds configured maximum**
    - For any sequence of consecutive failures, assert the sniper makes at most `config.max_buy_retries` buy attempts and `config.max_sell_retries` sell attempts
    - Use a mock executor that always returns `confirmed: false`; count invocations
    - Place in `scematica-sniper/src/sniper.rs` under `#[cfg(test)]`
    - **Validates: Requirements 12.2, 12.3**

- [~] 10. Final checkpoint — ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- The Meteora discriminator bug (`0x44` vs `0x43` as the last byte) must be fixed in task 4.4 — this is a correctness issue, not optional
- `get_builder` is a breaking change: every call site (`scematica-sniper/src/sniper.rs` and `scematica-arb/src/executor.rs`) must be updated in the same compilation unit as the factory change
- The `SellMonitor` struct mirrors `Sniper` fields — any field added to `Sniper` for Jupiter must also be added to `SellMonitor` and propagated through `clone_for_sell`
- Property tests use `proptest = "1"` as a dev-dependency; they run with `cargo test` and do not require network access
- Integration tests (devnet/mainnet RPC) are out of scope for this task list and should be gated behind `#[cfg(feature = "integration-tests")]`
- Each task references specific requirements for traceability
- Checkpoints ensure incremental validation

## Task Dependency Graph

```json
{
  "waves": [
    { "id": 0, "tasks": ["1", "9.1"] },
    { "id": 1, "tasks": ["2.1", "3.1", "4.1", "6.1", "6.2", "8.1"] },
    { "id": 2, "tasks": ["2.2", "3.2", "4.2"] },
    { "id": 3, "tasks": ["2.3", "3.3", "4.3"] },
    { "id": 4, "tasks": ["2.4", "3.4", "4.4"] },
    { "id": 5, "tasks": ["2.5", "2.6", "3.5", "3.6", "4.5", "6.3", "9.2"] },
    { "id": 6, "tasks": ["7.1"] },
    { "id": 7, "tasks": ["7.2", "7.3", "7.4", "7.5"] },
    { "id": 8, "tasks": ["7.6", "9.3"] }
  ]
}
```
