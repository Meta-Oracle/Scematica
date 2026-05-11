# Requirements Document

## Introduction

This feature completes the end-to-end trading execution pipeline for the Scematica Solana bot. The four broken execution paths — Raydium AMM V4, Orca Whirlpool, Meteora DLMM, and Jupiter versioned transactions — must be fixed so that swap instructions are built with real on-chain account addresses rather than `Pubkey::default()` placeholders. The wallet keypair is already loading correctly. This feature is purely about making swap instruction construction correct so that transactions land on-chain.

## Glossary

- **RaydiumBuilder**: The Rust struct in `scematica-executor/src/raydium.rs` responsible for building Raydium AMM V4 swap instructions.
- **OrcaBuilder**: The Rust struct in `scematica-executor/src/orca.rs` responsible for building Orca Whirlpool swap instructions.
- **MeteoraBuilder**: The Rust struct in `scematica-executor/src/meteora.rs` responsible for building Meteora DLMM swap instructions.
- **JupiterBuilder**: The Rust struct in `scematica-executor/src/jupiter.rs` responsible for obtaining Jupiter V6 versioned swap transactions.
- **Sniper**: The Rust struct in `scematica-sniper/src/sniper.rs` that orchestrates pool detection, filtering, and trade execution.
- **SwapInstructionBuilder**: The async trait in `scematica-executor/src/lib.rs` that all DEX builders implement.
- **RpcClient**: The Solana non-blocking RPC client used to fetch on-chain account data.
- **RaydiumAmmV4**: The Borsh-deserializable struct representing the Raydium AMM V4 pool state account (752 bytes).
- **SerumMarket**: The fixed-layout Serum/OpenBook market state account associated with a Raydium pool.
- **WhirlpoolState**: The Anchor-serialized Orca Whirlpool pool state account (272 bytes).
- **DlmmPoolState**: The Anchor-serialized Meteora DLMM LbPair pool state account.
- **VersionedTransaction**: A Solana `solana_sdk::transaction::VersionedTransaction` — the transaction format required by Jupiter V6.
- **TxExecutor**: The trait in `scematica-sniper/src/executor.rs` that signs and submits transactions to the Solana network.
- **ExecResult**: The result struct returned by `TxExecutor::execute`, containing `confirmed: bool`, `signature: Option<String>`, and `error: Option<String>`.
- **ATA**: Associated Token Account — the deterministic SPL token account for a given wallet and mint.
- **AMM authority**: The PDA derived from `[b"amm authority"]` and the Raydium AMM V4 program ID, used as the pool's signing authority.
- **Vault signer**: The Serum/OpenBook market PDA derived from the market ID and vault signer nonce, used to authorize vault transfers.
- **Tick array**: An Orca Whirlpool account that stores tick data for a range of price ticks; three are required per swap.
- **Bin array**: A Meteora DLMM account that stores liquidity data for a range of price bins; two are required per swap.
- **a_to_b**: Boolean flag in Orca Whirlpool swap instructions indicating whether the swap goes from token A to token B.
- **swap_for_y**: Boolean flag in Meteora DLMM swap instructions indicating whether the swap goes from token X to token Y.

## Requirements

### Requirement 1: RaydiumBuilder Accepts RPC Client

**User Story:** As a developer, I want the RaydiumBuilder to hold an RPC client, so that it can fetch on-chain pool state when building swap instructions.

#### Acceptance Criteria

1. THE `RaydiumBuilder::new` constructor SHALL accept exactly one parameter: an `Arc<RpcClient>`.
2. THE RaydiumBuilder SHALL store the `Arc<RpcClient>` as a private field accessible to `build_swap`.
3. WHEN `RaydiumBuilder::new(rpc)` is called, THE RaydiumBuilder SHALL not invoke any method on the `Arc<RpcClient>` during construction.
4. THE `get_builder` factory function in `scematica-executor/src/lib.rs` SHALL accept an `Arc<RpcClient>` as a second parameter, pass it to `RaydiumBuilder::new` for `DexKind::Raydium`, pass it to `OrcaBuilder::new` for `DexKind::Orca`, pass it to `MeteoraBuilder::new` for `DexKind::Meteora`, and accept but not forward it for `DexKind::Jupiter`.

---

### Requirement 2: RaydiumBuilder Fetches and Decodes Pool State

**User Story:** As a trader, I want the Raydium swap instruction to use real on-chain account addresses, so that the transaction is accepted by the Solana network.

#### Acceptance Criteria

1. WHEN `RaydiumBuilder::build_swap` is called, THE RaydiumBuilder SHALL call `rpc.get_account_data(pool)` to fetch the pool state.
2. WHEN the pool account data is fetched, THE RaydiumBuilder SHALL deserialize it into a `RaydiumAmmV4` struct using `BorshDeserialize`.
3. IF the fetched pool account data is fewer than 752 bytes, THEN THE RaydiumBuilder SHALL return an `Err` containing the actual byte length.
4. IF `rpc.get_account_data(pool)` returns an error, THEN THE RaydiumBuilder SHALL return an `Err` containing the pool pubkey.
5. WHEN the `RaydiumAmmV4` struct is decoded and `state.status == 0`, THEN THE RaydiumBuilder SHALL return an `Err` indicating the pool is uninitialized.
6. WHEN the `RaydiumAmmV4` struct is decoded successfully, THE RaydiumBuilder SHALL call `rpc.get_account_data(state.market_id)` to fetch the associated Serum/OpenBook market state.
7. IF `rpc.get_account_data(state.market_id)` returns an error, THEN THE RaydiumBuilder SHALL return an `Err` containing the market pubkey.
8. IF the fetched Serum market account data is fewer than 388 bytes, THEN THE RaydiumBuilder SHALL return an `Err` containing the actual byte length.
9. WHEN the Serum market data is at least 388 bytes, THE RaydiumBuilder SHALL extract `vault_signer_nonce` from bytes `[5+32..5+40]`, `event_queue` from bytes `[5+240..5+272]`, `bids` from bytes `[5+272..5+304]`, `asks` from bytes `[5+304..5+336]`, `coin_vault` from bytes `[5+104..5+136]`, and `pc_vault` from bytes `[5+152..5+184]`.
10. WHEN the vault signer nonce is extracted, THE RaydiumBuilder SHALL derive the vault signer PDA using `Pubkey::create_program_address(&[market_id.as_ref(), &nonce.to_le_bytes()], &state.market_program_id)`.
11. IF `Pubkey::create_program_address` fails for the vault signer derivation, THEN THE RaydiumBuilder SHALL return an `Err`.

---

### Requirement 3: RaydiumBuilder Produces a Valid 18-Account Swap Instruction

**User Story:** As a trader, I want the Raydium swap instruction to have all 18 required accounts populated with real pubkeys, so that the transaction is not rejected on-chain.

#### Acceptance Criteria

1. WHEN `RaydiumBuilder::build_swap` succeeds, THE RaydiumBuilder SHALL return a `Vec<Instruction>` containing exactly one instruction.
2. THE returned Raydium swap instruction SHALL have exactly 18 account metas.
3. THE returned Raydium swap instruction SHALL have `program_id == RAYDIUM_AMM_V4`.
4. THE returned Raydium swap instruction data SHALL begin with the byte `9u8` (swap base-in discriminator).
5. THE returned Raydium swap instruction data SHALL encode `amount_in` as a little-endian `u64` at bytes `[1..9]`.
6. THE returned Raydium swap instruction data SHALL encode `min_amount_out` as a little-endian `u64` at bytes `[9..17]`.
7. WHEN `RaydiumBuilder::build_swap` succeeds, THE returned instruction SHALL contain no account meta where the pubkey equals `Pubkey::default()`.
8. THE AMM authority account (index 2) SHALL be the PDA derived from `[b"amm authority"]` and `RAYDIUM_AMM_V4`.
9. THE open orders account (index 3) SHALL equal `state.open_orders` from the decoded `RaydiumAmmV4` struct.
10. THE target orders account (index 4) SHALL equal `state.target_orders` from the decoded `RaydiumAmmV4` struct.
11. THE base vault account (index 5) SHALL equal `state.base_vault` from the decoded `RaydiumAmmV4` struct.
12. THE quote vault account (index 6) SHALL equal `state.quote_vault` from the decoded `RaydiumAmmV4` struct.
13. THE serum program account (index 7) SHALL equal `state.market_program_id` from the decoded `RaydiumAmmV4` struct.
14. THE serum market account (index 8) SHALL equal `state.market_id` from the decoded `RaydiumAmmV4` struct.
15. THE serum bids (index 9), asks (index 10), event queue (index 11), coin vault (index 12), pc vault (index 13), and vault signer (index 14) SHALL be sourced from the decoded Serum market account data.
16. IF any pool state or serum market account cannot be resolved, THEN THE RaydiumBuilder SHALL return an `Err` rather than a partial instruction.

---

### Requirement 4: OrcaBuilder Accepts RPC Client and Fetches Whirlpool State

**User Story:** As a trader, I want the Orca swap instruction to use real vault addresses and correct swap direction, so that the transaction is accepted by the Solana network.

#### Acceptance Criteria

1. THE OrcaBuilder SHALL accept an `Arc<RpcClient>` parameter in its constructor and store it as a private field.
2. WHEN `OrcaBuilder::build_swap` is called, THE OrcaBuilder SHALL call `rpc.get_account_data(pool)` to fetch the Whirlpool state.
3. IF `rpc.get_account_data(pool)` returns an error or the data is fewer than 272 bytes, THEN THE OrcaBuilder SHALL return an `Err`.
4. WHEN the Whirlpool state is decoded, THE OrcaBuilder SHALL extract the following fields by skipping the 8-byte Anchor discriminator: `tick_spacing` at offset `8+9` (u16), `tick_current_index` at offset `8+37` (i32), `token_mint_a` at offset `8+101` (Pubkey), `token_vault_a` at offset `8+133` (Pubkey), `token_mint_b` at offset `8+181` (Pubkey), `token_vault_b` at offset `8+213` (Pubkey).
5. WHEN the Whirlpool state is decoded, THE OrcaBuilder SHALL set `a_to_b = (token_in == token_mint_a)`.
6. IF `token_in` equals neither `token_mint_a` nor `token_mint_b`, THEN THE OrcaBuilder SHALL return an `Err`.
7. WHEN `a_to_b` is `true`, THE OrcaBuilder SHALL set `sqrt_price_limit` to `4295048016u128` (MIN_SQRT_PRICE).
8. WHEN `a_to_b` is `false`, THE OrcaBuilder SHALL set `sqrt_price_limit` to `79226673515401279992447579055u128` (MAX_SQRT_PRICE).
9. WHEN `OrcaBuilder::build_swap` is called, THE OrcaBuilder SHALL derive the oracle PDA using `Pubkey::find_program_address(&[b"oracle", pool.as_ref()], &ORCA_WHIRLPOOL)`.
10. WHEN `OrcaBuilder::build_swap` is called, THE OrcaBuilder SHALL derive 3 tick array PDAs using `start_tick_index = (tick_current_index.div_euclid(tick_spacing as i32 * 88) + offset) * (tick_spacing as i32 * 88)` where offsets are `0`, `-1` (if `a_to_b`) or `+1` (if `!a_to_b`), and `-2` (if `a_to_b`) or `+2` (if `!a_to_b`), and each PDA is derived via `Pubkey::find_program_address(&[b"tick_array", pool.as_ref(), start_tick.to_string().as_bytes()], &ORCA_WHIRLPOOL)`.
11. WHEN `a_to_b` is `true`, THE OrcaBuilder SHALL assign `ata_in` as `user_token_a` and `ata_out` as `user_token_b`; WHEN `a_to_b` is `false`, THE OrcaBuilder SHALL assign `ata_out` as `user_token_a` and `ata_in` as `user_token_b`.
12. THE 11 accounts SHALL be ordered as: `[spl_token (readonly), owner (signer), pool (writable), user_token_a (writable), token_vault_a (writable), user_token_b (writable), token_vault_b (writable), tick_array_0 (writable), tick_array_1 (writable), tick_array_2 (writable), oracle (readonly)]`.

---

### Requirement 5: OrcaBuilder Produces a Valid 11-Account Swap Instruction

**User Story:** As a trader, I want the Orca swap instruction to have all 11 required accounts populated with real pubkeys, so that the transaction is not rejected on-chain.

#### Acceptance Criteria

1. WHEN `OrcaBuilder::build_swap` succeeds, THE OrcaBuilder SHALL return a `Vec<Instruction>` containing exactly one instruction.
2. THE returned Orca swap instruction SHALL have exactly 11 account metas.
3. THE returned Orca swap instruction SHALL have `program_id == ORCA_WHIRLPOOL`.
4. THE returned Orca swap instruction data SHALL begin with the 8-byte Whirlpool swap discriminator `[0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x75, 0x27, 0x43]`.
5. WHEN `OrcaBuilder::build_swap` succeeds, THE returned instruction SHALL contain no account meta where the pubkey equals `Pubkey::default()`.
6. THE token vault A account (index 4) SHALL equal `token_vault_a` from the decoded Whirlpool state.
7. THE token vault B account (index 6) SHALL equal `token_vault_b` from the decoded Whirlpool state.
8. THE oracle account (index 10) SHALL equal the PDA derived from `[b"oracle", pool.as_ref()]` and `ORCA_WHIRLPOOL`.
9. THE tick array accounts (indices 7, 8, 9) SHALL be the 3 PDAs derived from `tick_current_index`, `tick_spacing`, and swap direction as specified in Requirement 4.10.
10. IF `rpc.get_account_data(pool)` fails or returns fewer than 272 bytes, THEN `OrcaBuilder::build_swap` SHALL return an `Err` rather than a partial instruction.

---

### Requirement 6: MeteoraBuilder Accepts RPC Client and Fetches DLMM Pool State

**User Story:** As a trader, I want the Meteora swap instruction to use real reserve accounts and correct swap direction, so that the transaction is accepted by the Solana network.

#### Acceptance Criteria

1. THE MeteoraBuilder SHALL accept an `Arc<RpcClient>` parameter in its constructor and store it as a private field.
2. WHEN `MeteoraBuilder::build_swap` is called, THE MeteoraBuilder SHALL call `rpc.get_account_data(pool)` to fetch the DLMM LbPair state.
3. IF `rpc.get_account_data(pool)` returns an error or the data is too short to decode all required fields, THEN THE MeteoraBuilder SHALL return an `Err`.
4. WHEN the DLMM state is decoded, THE MeteoraBuilder SHALL extract `reserve_x`, `reserve_y`, `token_x_mint`, `token_y_mint`, `active_id`, `bin_step`, `oracle`, and `bin_array_bitmap_extension` from the Anchor-serialized layout (skipping the 8-byte discriminator).
5. WHEN the DLMM state is decoded, THE MeteoraBuilder SHALL set `swap_for_y = (token_in == token_x_mint)`.
6. IF `token_in` equals neither `token_x_mint` nor `token_y_mint`, THEN THE MeteoraBuilder SHALL return an `Err`.
7. WHEN `MeteoraBuilder::build_swap` is called, THE MeteoraBuilder SHALL derive 2 bin array PDAs using `bin_array_index = active_id.div_euclid(70)` and `bin_array_index + 1`, each via `Pubkey::find_program_address(&[b"bin_array", pool.as_ref(), &(index as i64).to_le_bytes()], &METEORA_DLMM)`.

---

### Requirement 7: MeteoraBuilder Produces a Valid 14-Account Swap Instruction

**User Story:** As a trader, I want the Meteora swap instruction to have all 14 required accounts populated with real pubkeys, so that the transaction is not rejected on-chain.

#### Acceptance Criteria

1. WHEN `MeteoraBuilder::build_swap` succeeds, THE MeteoraBuilder SHALL return a `Vec<Instruction>` containing exactly one instruction.
2. THE returned Meteora swap instruction SHALL have exactly 14 account metas.
3. THE returned Meteora swap instruction SHALL have `program_id == METEORA_DLMM`.
4. THE returned Meteora swap instruction data SHALL begin with the 8-byte Meteora swap discriminator `[0xf8, 0xc6, 0x9e, 0x91, 0xe1, 0x75, 0x27, 0x43]` followed by `amount_in` (u64 LE), `min_amount_out` (u64 LE), and `swap_for_y` (1u8 if true, 0u8 if false), totalling 25 bytes.
5. WHEN `MeteoraBuilder::build_swap` succeeds, THE returned instruction SHALL contain no account meta where the pubkey equals `Pubkey::default()`.
6. THE reserve X account (index 2) SHALL equal `reserve_x` from the decoded DLMM state.
7. THE reserve Y account (index 3) SHALL equal `reserve_y` from the decoded DLMM state.
8. THE oracle account (index 8) SHALL equal `oracle` from the decoded DLMM state.
9. THE bin array accounts (indices 9 and 10) SHALL be the 2 PDAs derived from `active_id` as specified in Requirement 6.7.
10. IF any required field cannot be decoded from the DLMM state, THEN THE MeteoraBuilder SHALL return an `Err` rather than a partial instruction.

---

### Requirement 8: JupiterBuilder Deserializes Versioned Transactions

**User Story:** As a trader, I want the Jupiter swap path to produce a valid `VersionedTransaction`, so that it can be re-signed and submitted to the Solana network.

#### Acceptance Criteria

1. THE JupiterBuilder SHALL expose a `deserialize_transaction(tx_bytes: &[u8]) -> Result<VersionedTransaction>` method.
2. WHEN `deserialize_transaction` is called with valid bincode-serialized `VersionedTransaction` bytes, THE JupiterBuilder SHALL return a `VersionedTransaction` without error.
3. IF `deserialize_transaction` is called with malformed or empty bytes, THEN THE JupiterBuilder SHALL return an `Err`.
4. WHEN `get_swap_transaction` is called and the Jupiter API response does not contain a `swapTransaction` field, THEN THE JupiterBuilder SHALL return an `Err` with the message `"No swapTransaction in Jupiter response"`.
5. WHEN `get_swap_transaction` is called and the Jupiter API returns a non-200 HTTP status, THEN THE JupiterBuilder SHALL return an `Err` containing the HTTP status code.
6. THE `build_swap` method on `JupiterBuilder` SHALL continue to return an empty `Vec<Instruction>` and emit a `tracing::warn!` directing callers to use `get_swap_transaction` instead.

---

### Requirement 9: Sniper Routes Jupiter Pools Through get_swap_transaction

**User Story:** As a trader, I want the sniper to use Jupiter's versioned transaction path for Jupiter-routed swaps, so that the transaction is correctly formed and lands on-chain.

#### Acceptance Criteria

1. WHEN the sniper executes a buy for a pool routed through Jupiter, THE Sniper SHALL call `JupiterBuilder::get_quote` followed by `JupiterBuilder::get_swap_transaction` instead of `SwapInstructionBuilder::build_swap`.
2. WHEN `get_swap_transaction` returns transaction bytes, THE Sniper SHALL call `JupiterBuilder::deserialize_transaction` to obtain a `VersionedTransaction`.
3. WHEN a `VersionedTransaction` is obtained from Jupiter, THE Sniper SHALL fetch a fresh blockhash via `rpc.get_latest_blockhash()` and re-sign the transaction with the wallet keypair before submission.
4. WHEN the Jupiter `VersionedTransaction` is signed, THE Sniper SHALL submit it via `rpc.send_and_confirm_transaction`.
5. IF any step in the Jupiter swap pipeline fails, THEN THE Sniper SHALL log the error at `error!` level and retry up to `config.max_buy_retries` total attempts.

---

### Requirement 10: Builder Factory Updated for RPC Injection

**User Story:** As a developer, I want the `get_builder` factory to accept an RPC client, so that all builders that require on-chain data can be constructed correctly.

#### Acceptance Criteria

1. THE `get_builder` function SHALL accept an `Arc<RpcClient>` as a second parameter.
2. WHEN `get_builder(DexKind::Raydium, rpc)` is called, THE factory SHALL return a `RaydiumBuilder` constructed with the provided `Arc<RpcClient>`.
3. WHEN `get_builder(DexKind::Orca, rpc)` is called, THE factory SHALL return an `OrcaBuilder` constructed with the provided `Arc<RpcClient>`.
4. WHEN `get_builder(DexKind::Meteora, rpc)` is called, THE factory SHALL return a `MeteoraBuilder` constructed with the provided `Arc<RpcClient>`.
5. WHEN `get_builder(DexKind::Jupiter, rpc)` is called, THE factory SHALL return a `JupiterBuilder` constructed without RPC (Jupiter uses HTTP, not RPC).
6. THE Sniper SHALL pass its `Arc<RpcClient>` to `get_builder` when constructing builders at startup.

---

### Requirement 11: Swap Instruction Data Encoding

**User Story:** As a developer, I want the swap instruction data to be correctly encoded for each DEX, so that the on-chain program can parse the instruction without error.

#### Acceptance Criteria

1. THE `raydium_swap_data` function SHALL produce a 17-byte `Vec<u8>` where byte `[0]` is `9u8`, bytes `[1..9]` are `amount_in` in little-endian u64, and bytes `[9..17]` are `min_amount_out` in little-endian u64.
2. THE Orca swap instruction data SHALL be exactly 42 bytes: 8-byte discriminator, then `amount_in` (u64 LE), `min_amount_out` (u64 LE), `sqrt_price_limit` (u128 LE), `amount_specified_is_input` (fixed `1u8`), and `a_to_b` (`1u8` if true, `0u8` if false).
3. THE Meteora swap instruction data SHALL be exactly 25 bytes: 8-byte discriminator, then `amount_in` (u64 LE), `min_amount_out` (u64 LE), and `swap_for_y` (`1u8` if true, `0u8` if false).

---

### Requirement 12: Error Propagation and Retry Behaviour

**User Story:** As a trader, I want swap errors to be propagated correctly and retried by the sniper, so that transient RPC failures do not permanently block a trade.

#### Acceptance Criteria

1. WHEN `build_swap` returns an `Err`, THE Sniper SHALL log the error at `error!` level and count it as one failed attempt without panicking.
2. WHEN a buy attempt fails, THE Sniper SHALL make at most `config.max_buy_retries` total attempts before calling `metrics.record_trade_failed()` once after all attempts are exhausted.
3. WHEN a sell attempt fails, THE Sniper SHALL make at most `config.max_sell_retries` total attempts before calling `metrics.record_trade_failed()` once after all attempts are exhausted.
4. WHEN `build_swap` returns `Ok` with an empty `Vec<Instruction>`, THE Sniper SHALL not submit a transaction and SHALL treat the attempt as failed, incrementing the retry counter.
5. WHEN `one_token_at_a_time` is `true` and all buy retries are exhausted, THE Sniper SHALL release the `processing_lock` by setting it to `false`.

---

### Requirement 13: Slippage Applied to Buy Instructions

**User Story:** As a trader, I want the buy instruction to use a real minimum output amount derived from the configured slippage, so that I am protected from excessive price impact.

#### Acceptance Criteria

1. WHEN building a buy swap instruction, THE Sniper SHALL compute `min_amount_out` as `apply_slippage((amount * quote_reserve / base_reserve) as u64, config.buy_slippage_pct)` using the pool's quote and base vault balances as the AMM reserve estimate.
2. WHEN calling `build_swap` in the buy path, THE Sniper SHALL pass the computed `min_amount_out` value and SHALL NOT pass `0`.
3. WHEN building a sell swap instruction, THE Sniper SHALL compute `min_amount_out` by applying `config.sell_slippage_pct` to the estimated output amount using `apply_slippage`.
