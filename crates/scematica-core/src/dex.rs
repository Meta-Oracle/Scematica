use crate::types::DexKind;
use solana_sdk::pubkey::Pubkey;

/// Well-known program IDs for each DEX
pub mod program_ids {
    use solana_sdk::pubkey;
    use solana_sdk::pubkey::Pubkey;

    pub const RAYDIUM_AMM_V4: Pubkey =
        pubkey!("675kPX9MHTjS2zt1qfr1NYHuzeLXfQM9H24wFSUt1Mp8");
    pub const RAYDIUM_CPMM: Pubkey =
        pubkey!("CPMMoo8L3F4NbTegBCKVNunggL7H1ZpdTHKxQB5qKP1C");
    pub const ORCA_WHIRLPOOL: Pubkey =
        pubkey!("whirLbMiicVdio4qvUfM5KAg6Ct8VwpYzGff3uctyCc");
    pub const METEORA_DLMM: Pubkey =
        pubkey!("LBUZKhRxPF3XUpBCjp4YzTKgLccjZhTSDM9YuVaPwxo");
    pub const METEORA_DAMM: Pubkey =
        pubkey!("Eo7WjKq67rjJQSZxS6z3YkapzY3eMj6Xy8X5EQVn5UaB");
    pub const PUMP_FUN: Pubkey =
        pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
    pub const JUPITER_V6: Pubkey =
        pubkey!("JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4");
    pub const OPENBOOK_V2: Pubkey =
        pubkey!("opnb2LAfJYbRMAHHvqjCwQxanZn7ReEHp1k81EohpZb");
}

/// Identify which DEX a program ID belongs to
pub fn identify_dex(program_id: &Pubkey) -> DexKind {
    match *program_id {
        p if p == program_ids::RAYDIUM_AMM_V4 => DexKind::Raydium,
        p if p == program_ids::RAYDIUM_CPMM => DexKind::Raydium,
        p if p == program_ids::ORCA_WHIRLPOOL => DexKind::Orca,
        p if p == program_ids::METEORA_DLMM => DexKind::Meteora,
        p if p == program_ids::METEORA_DAMM => DexKind::Meteora,
        p if p == program_ids::PUMP_FUN => DexKind::PumpFun,
        p if p == program_ids::JUPITER_V6 => DexKind::Jupiter,
        _ => DexKind::Unknown,
    }
}

/// Raydium AMM V4 pool state layout constants (verified against on-chain data).
///
/// The pre-Pubkey section is 336 bytes:
///   16 u64s (status…sysDecimalValue)   = 128
///   8  u64s (fees)                      =  64
///   4  u64s (needTakePnl…totalPnl)     =  32
///   1  u128 (poolOpenTime)              =  16
///   3  u64s (punish×2, orderbookInit)  =  24
///   2  u128 (swapCoinIn, swapPcOut)    =  32
///   1  u64  (swapTakePcFee)            =   8
///   2  u128 (swapPcIn, swapCoinOut)    =  32
///                              total  = 336
pub mod raydium_v4 {
    pub const POOL_STATE_SIZE: usize = 752;
    pub const STATUS_OFFSET: usize = 0;
    pub const BASE_VAULT_OFFSET: usize = 336;     // tokenCoin
    pub const QUOTE_VAULT_OFFSET: usize = 368;    // tokenPc
    pub const BASE_MINT_OFFSET: usize = 400;      // coinMint
    pub const QUOTE_MINT_OFFSET: usize = 432;     // pcMint
    pub const LP_MINT_OFFSET: usize = 464;        // lpMint
    pub const OPEN_ORDERS_OFFSET: usize = 496;    // openOrders
    pub const MARKET_ID_OFFSET: usize = 528;      // market (was wrong: 464)
    pub const MARKET_PROGRAM_OFFSET: usize = 560; // marketProgram
    pub const TARGET_ORDERS_OFFSET: usize = 592;  // targetOrders
    pub const OPEN_TIME_OFFSET: usize = 224;      // poolOpenTime (u128 lo-bits, was wrong: 200)
}

/// Raydium V4 market state layout constants (OpenBook/Serum)
pub mod raydium_market {
    pub const MARKET_STATE_SIZE: usize = 388;
}
