use anchor_lang::prelude::*;
use anchor_spl::token::{Token, TokenAccount};

// Rotated 2026-08-13. The previous program keypair was committed to git history (see
// DEPLOY_DEVNET.md) and is therefore public — anyone holding it could have squatted the
// address before our deploy landed. Its replacement lives only in target/deploy/,
// which `.gitignore` covers via `programs/*/target/`.
declare_id!("7rRHfgQphASzDTGEyLUEsh9daZ2hRXZA1GP9MPvSxXBh");

/// Scematica on-chain swap program.
///
/// Implements the profit-or-revert pattern from 0xNineteen/solana-arbitrage-bot:
/// 1. `start_swap`      — records the initial token balance in a PDA
/// 2. (N swap CPI calls happen in the same transaction, built by the client)
/// 3. `profit_or_revert` — checks final balance > initial balance, else reverts
///
/// This guarantees atomicity: either the arb is profitable and executes,
/// or the entire transaction reverts and no funds are lost (minus tx fee).
#[program]
pub mod scematica_swap {
    use super::*;

    /// Initialize the swap state PDA and record the starting token balance.
    /// Called as the first instruction in an arb transaction.
    pub fn start_swap(ctx: Context<StartSwap>, swap_input: u64, min_expected_output: u64) -> Result<()> {
        let swap_state = &mut ctx.accounts.swap_state;
        let initial_balance = ctx.accounts.src.amount;
        let min_final_balance = initial_balance
            .saturating_sub(swap_input)
            .saturating_add(min_expected_output);

        swap_state.swap_input = initial_balance;
        swap_state.expected_output = min_final_balance;
        swap_state.authority = ctx.accounts.authority.key();
        swap_state.src_mint = ctx.accounts.src.mint;
        swap_state.src = ctx.accounts.src.key();

        msg!(
            "StartSwap: mint={} input={} min_output={} from {}",
            swap_state.src_mint,
            initial_balance,
            min_final_balance,
            ctx.accounts.src.key()
        );
        Ok(())
    }

    /// Assert that the final token balance is greater than the initial balance
    /// and meets the minimum expected output.
    /// If not, the instruction fails and the entire transaction reverts.
    pub fn profit_or_revert(ctx: Context<ProfitOrRevert>) -> Result<()> {
        let swap_state = &ctx.accounts.swap_state;
        let final_balance = ctx.accounts.src.amount;
        let initial_input = swap_state.swap_input;
        let min_output = swap_state.expected_output;

        msg!(
            "ProfitOrRevert: mint={} initial={} final={} min_required={}",
            swap_state.src_mint,
            initial_input,
            final_balance,
            min_output
        );

        // Security: the balance must be read from the account the baseline was measured
        // on, not merely from an account of the same mint.
        require_keys_eq!(
            ctx.accounts.src.key(),
            swap_state.src,
            ScematicaError::InvalidTokenAccount
        );
        // Kept as well as, not instead of: the mint is what the reported profit is
        // denominated in, and an account cannot change mint, so this is now redundant
        // and costs one comparison to stay obvious.
        require_keys_eq!(
            ctx.accounts.src.mint,
            swap_state.src_mint,
            ScematicaError::InvalidTokenAccount
        );

        // Check absolute profit
        require!(
            final_balance > initial_input,
            ScematicaError::NotProfitable
        );

        // Check slippage / expected output
        require!(
            final_balance >= min_output,
            ScematicaError::SlippageExceeded
        );

        let profit = final_balance - initial_input;
        msg!("Profit: {} lamports", profit);

        Ok(())
    }
}

/// Swap state PDA — stores the initial balance for profit verification
#[account]
#[derive(Default)]
pub struct SwapState {
    /// The authority (wallet) that initiated the swap
    pub authority: Pubkey,
    /// The mint of the token being swapped
    pub src_mint: Pubkey,
    /// The token **account** the baseline was measured on.
    ///
    /// The state used to record only the mint, so `profit_or_revert` could not tell two
    /// accounts of the same mint apart: it re-read `amount` from whatever account it was
    /// handed. A bundle assembled with the wrong `src` got a green light from an account
    /// that never took part in the trade, while the account that did could be down on the
    /// day. Not exploitable by a third party — only the recorded `authority` can invoke
    /// the check, and a failure reverts that authority's own transaction — but the guard
    /// was proving less than "this arbitrage was profitable", which is the only thing it
    /// is there to prove.
    pub src: Pubkey,
    /// Input amount at the start of the swap
    pub swap_input: u64,
    /// Expected minimum output (set by client, verified by profit_or_revert)
    pub expected_output: u64,
    /// Bump seed for PDA derivation
    pub bump: u8,
}

impl SwapState {
    // discriminator + authority + src_mint + src + swap_input + expected_output + bump
    pub const LEN: usize = 8 + 32 + 32 + 32 + 8 + 8 + 1;
}

/// Accounts for start_swap
#[derive(Accounts)]
pub struct StartSwap<'info> {
    /// The source token account (holds the starting capital)
    #[account(mut)]
    pub src: Account<'info, TokenAccount>,

    /// Swap state PDA — created per authority
    #[account(
        init_if_needed,
        payer = authority,
        space = SwapState::LEN,
        seeds = [b"swap_state", authority.key().as_ref()],
        bump,
    )]
    pub swap_state: Account<'info, SwapState>,

    /// The wallet signing the transaction
    #[account(mut)]
    pub authority: Signer<'info>,

    pub system_program: Program<'info, System>,
    pub token_program: Program<'info, Token>,
}

/// Accounts for profit_or_revert
#[derive(Accounts)]
pub struct ProfitOrRevert<'info> {
    /// The source token account — checked for final balance
    pub src: Account<'info, TokenAccount>,

    /// Swap state PDA — read to get the baseline, and **closed here**.
    ///
    /// The state was opened with `init_if_needed` and never closed, so a baseline
    /// survived the transaction that set it: `profit_or_revert` did not require that
    /// `start_swap` had run alongside it, and a stale, lower baseline keeps being
    /// satisfied by any later balance. Whether the check meant anything was a property of
    /// how the client ordered its instructions rather than of the program. Closing the
    /// state as it is consumed makes a baseline usable exactly once, so `init_if_needed`
    /// now finds nothing to reuse on the next round trip.
    #[account(
        mut,
        close = authority,
        seeds = [b"swap_state", authority.key().as_ref()],
        bump,
        has_one = authority,
    )]
    pub swap_state: Account<'info, SwapState>,

    /// The wallet that initiated the swap
    #[account(mut)]
    pub authority: Signer<'info>,
}

#[error_code]
pub enum ScematicaError {
    #[msg("Swap was not profitable: final balance did not exceed initial input")]
    NotProfitable,

    #[msg("Slippage exceeded: final balance was below minimum expected output")]
    SlippageExceeded,

    #[msg("Unauthorized: signer does not match swap state authority")]
    Unauthorized,

    #[msg("Invalid token account: mint mismatch")]
    InvalidTokenAccount,
}
