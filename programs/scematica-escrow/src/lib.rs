use anchor_lang::prelude::*;
use anchor_spl::token::{self, CloseAccount, Mint, Token, TokenAccount, Transfer};

declare_id!("Fu5nDuRRBTTJGNBMcFC1hHvBQybtiCECeNzUBRHmVwLz");

/// # ScemaDEX optimistic bond escrow
///
/// Trustless, on-chain custody for a Conviction-Routing bond — the mainnet analog of
/// [`scemadex_sdk::SettlementMachine`]. No facilitator ever holds the funds: the bond
/// (and any challenge stake) live in a program-owned vault until the lifecycle
/// reaches **Finalized**, at which point `settle` moves money and closes the accounts.
///
/// ```text
/// escrow ─▶ Escrowed ─mark_provisional─▶ Provisional ─┬─ finalize_timeout (window) ─▶ Finalized(Honored)
///    │                                                 └─ file_challenge ─▶ Disputed ─resolve─▶ Finalized(Honored|Slashed)
///    └──────────────── finalize_timeout (deadline, no fill) ───────────────────────▶ Finalized(Slashed)
///                                                                                              │
///                                                                     settle ◀────────────────┘  (distributes + closes)
/// ```
///
/// **Optimistic finality.** Funds only move at `settle`, after `Finalized`. The
/// dispute window is the on-chain challenge period — the same shape as an optimistic
/// rollup — giving a challenger time to refute a bad inference before the money is
/// irreversible. A verified proof can also collapse the window off-chain, letting the
/// `authority` `resolve` immediately.
///
/// **Slash routing** mirrors the SDK's four-way split (caller / challengers /
/// insurance / lineage), in basis points that must sum to 10_000. The caller share
/// absorbs integer-division dust, so the full bond is always disbursed.
#[program]
pub mod scemadex_escrow {
    use super::*;

    /// Lock a bond. The agent transfers `bond_amount` of `mint` into the vault and
    /// the escrow PDA opens in `Escrowed`. `authority` is the facilitator/oracle
    /// permitted to `mark_provisional`/`resolve`; `caller` is the beneficiary on a
    /// slash. `digest` is the 32-byte intent digest (the PDA seed).
    #[allow(clippy::too_many_arguments)]
    pub fn escrow(
        ctx: Context<EscrowBond>,
        digest: [u8; 32],
        bond_amount: u64,
        min_out_raw: u64,
        deadline_unix: i64,
        dispute_window_secs: i64,
        routing: SlashRouting,
        authority: Pubkey,
        caller: Pubkey,
    ) -> Result<()> {
        require!(routing.is_valid(), EscrowError::InvalidRouting);
        require!(bond_amount > 0, EscrowError::ZeroBond);

        // Move the bond from the agent into the program vault.
        token::transfer(ctx.accounts.deposit_ctx(), bond_amount)?;

        let e = &mut ctx.accounts.escrow;
        e.agent = ctx.accounts.agent.key();
        e.caller = caller;
        e.mint = ctx.accounts.mint.key();
        e.vault = ctx.accounts.vault.key();
        e.digest = digest;
        e.bond_amount = bond_amount;
        e.min_out_raw = min_out_raw;
        e.deadline_unix = deadline_unix;
        e.dispute_window_secs = dispute_window_secs;
        e.window_closes_unix = 0;
        e.routing = routing;
        e.state = BondState::Escrowed as u8;
        e.provisional_honored = false;
        e.final_slashed = false;
        e.challenger = Pubkey::default();
        e.challenger_stake = 0;
        e.authority = authority;
        e.bump = ctx.bumps.escrow;

        msg!(
            "escrow: agent={} bond={} window={}s state=Escrowed",
            e.agent,
            bond_amount,
            dispute_window_secs
        );
        Ok(())
    }

    /// Record a fill's provisional outcome and open the dispute window. Only the
    /// `authority` (facilitator/oracle) may call it, from `Escrowed`.
    pub fn mark_provisional(ctx: Context<AuthorityOnly>, honored: bool) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let e = &mut ctx.accounts.escrow;
        require!(e.state == BondState::Escrowed as u8, EscrowError::BadState);
        e.state = BondState::Provisional as u8;
        e.provisional_honored = honored;
        e.window_closes_unix = now.saturating_add(e.dispute_window_secs);
        msg!(
            "mark_provisional: honored={} window_closes={} state=Provisional",
            honored,
            e.window_closes_unix
        );
        Ok(())
    }

    /// Stake against a provisionally-**honored** bond during the window, moving it to
    /// `Disputed`. The challenger transfers `stake` into the vault. Rejects a closed
    /// window, an already-slashed provisional, or a second challenger.
    pub fn file_challenge(ctx: Context<FileChallenge>, stake: u64) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        {
            let e = &ctx.accounts.escrow;
            require!(e.state == BondState::Provisional as u8, EscrowError::BadState);
            require!(e.provisional_honored, EscrowError::NothingToChallenge);
            require!(now < e.window_closes_unix, EscrowError::WindowClosed);
            require!(stake > 0, EscrowError::ZeroStake);
        }
        token::transfer(ctx.accounts.stake_ctx(), stake)?;
        let e = &mut ctx.accounts.escrow;
        e.state = BondState::Disputed as u8;
        e.challenger = ctx.accounts.challenger.key();
        e.challenger_stake = stake;
        msg!("file_challenge: challenger={} stake={} state=Disputed", e.challenger, stake);
        Ok(())
    }

    /// Resolve an open dispute with the adjudicated truth (oracle read or verified
    /// proof). `challenger_won == true` upholds the challenge and finalizes `Slashed`;
    /// otherwise the provisional honor stands. `authority`-only, from `Disputed`.
    pub fn resolve(ctx: Context<AuthorityOnly>, challenger_won: bool) -> Result<()> {
        let e = &mut ctx.accounts.escrow;
        require!(e.state == BondState::Disputed as u8, EscrowError::BadState);
        e.state = BondState::Finalized as u8;
        e.final_slashed = challenger_won;
        msg!("resolve: challenger_won={} state=Finalized", challenger_won);
        Ok(())
    }

    /// Permissionlessly finalize a matured bond: a `Provisional` bond whose window
    /// elapsed unchallenged honors; an `Escrowed` bond whose deadline passed with no
    /// fill slashes (failure to deliver). Errors while still live.
    pub fn finalize_timeout(ctx: Context<FinalizeTimeout>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let e = &mut ctx.accounts.escrow;
        match e.state {
            s if s == BondState::Provisional as u8 => {
                require!(now >= e.window_closes_unix, EscrowError::WindowOpen);
                e.final_slashed = false; // optimistic honor
            }
            s if s == BondState::Escrowed as u8 => {
                require!(
                    e.deadline_unix != 0 && now >= e.deadline_unix,
                    EscrowError::DeadlineNotPassed
                );
                e.final_slashed = true; // failure to deliver
            }
            _ => return err!(EscrowError::BadState),
        }
        e.state = BondState::Finalized as u8;
        msg!("finalize_timeout: slashed={} state=Finalized", e.final_slashed);
        Ok(())
    }

    /// Disburse a `Finalized` bond and close the vault + escrow. On **Honored**, the
    /// bond (and any forfeited challenge stake) returns to the agent. On **Slashed**,
    /// the bond splits four ways per `routing` and the challenger recovers its stake
    /// (it was right). Signed by the escrow PDA; permissionless once finalized.
    pub fn settle(ctx: Context<Settle>) -> Result<()> {
        require!(
            ctx.accounts.escrow.state == BondState::Finalized as u8,
            EscrowError::BadState
        );
        let e = &ctx.accounts.escrow;
        let bond = e.bond_amount;
        let stake = e.challenger_stake;
        let slashed = e.final_slashed;
        let routing = e.routing;
        let seeds: &[&[u8]] = &[b"escrow", e.digest.as_ref(), &[e.bump]];

        if !slashed {
            // Honored: return the bond to the agent; a losing challenger's stake is
            // forfeited to the agent as premium.
            let payout = bond.saturating_add(stake);
            transfer_from_vault(&ctx, ctx.accounts.agent_token.to_account_info(), payout, seeds)?;
            msg!("settle: Honored -> agent={} ({} bond + {} premium)", e.agent, bond, stake);
        } else {
            // Slashed: four-way split, caller absorbs dust. The challenger (if any)
            // recovers its stake plus receives the challenger slice.
            let d = routing.distribute(bond);
            transfer_from_vault(&ctx, ctx.accounts.caller_token.to_account_info(), d.caller, seeds)?;
            transfer_from_vault(&ctx, ctx.accounts.insurance_token.to_account_info(), d.insurance, seeds)?;
            transfer_from_vault(&ctx, ctx.accounts.lineage_token.to_account_info(), d.lineage, seeds)?;
            // Challenger slice + stake refund go to the challenger token account when a
            // challenger exists, else the challenger slice rolls to the caller.
            if stake > 0 || e.challenger != Pubkey::default() {
                transfer_from_vault(
                    &ctx,
                    ctx.accounts.challenger_token.to_account_info(),
                    d.challengers.saturating_add(stake),
                    seeds,
                )?;
            } else {
                transfer_from_vault(&ctx, ctx.accounts.caller_token.to_account_info(), d.challengers, seeds)?;
            }
            msg!(
                "settle: Slashed -> caller={} challengers={} insurance={} lineage={}",
                d.caller,
                d.challengers,
                d.insurance,
                d.lineage
            );
        }

        // Close the now-empty vault, refunding its rent to the agent.
        // `signer_seeds` must be a named binding: `&[seeds]` inline is a temporary that
        // is dropped at the end of the statement while the CpiContext still borrows it.
        let signer_seeds = [seeds];
        let cpi = CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            CloseAccount {
                account: ctx.accounts.vault.to_account_info(),
                destination: ctx.accounts.agent.to_account_info(),
                authority: ctx.accounts.escrow.to_account_info(),
            },
            &signer_seeds,
        );
        token::close_account(cpi)?;
        Ok(())
    }
}

/// A four-way split of a slashed bond, in basis points summing to 10_000. Mirrors
/// `scemadex_sdk::SlashRouting`.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Default)]
pub struct SlashRouting {
    pub to_caller_bps: u16,
    pub to_challengers_bps: u16,
    pub to_insurance_bps: u16,
    pub to_lineage_bps: u16,
}

/// The resolved split amounts (caller absorbs dust so the sum equals the bond).
pub struct SlashSplit {
    pub caller: u64,
    pub challengers: u64,
    pub insurance: u64,
    pub lineage: u64,
}

impl SlashRouting {
    pub fn is_valid(&self) -> bool {
        self.to_caller_bps as u32
            + self.to_challengers_bps as u32
            + self.to_insurance_bps as u32
            + self.to_lineage_bps as u32
            == 10_000
    }

    /// Split `bond` per the routing; the caller share takes the rounding remainder.
    pub fn distribute(&self, bond: u64) -> SlashSplit {
        let share = |bps: u16| ((bond as u128) * bps as u128 / 10_000u128) as u64;
        let challengers = share(self.to_challengers_bps);
        let insurance = share(self.to_insurance_bps);
        let lineage = share(self.to_lineage_bps);
        let caller = bond
            .saturating_sub(challengers)
            .saturating_sub(insurance)
            .saturating_sub(lineage);
        SlashSplit { caller, challengers, insurance, lineage }
    }
}

/// Lifecycle states, stored as a `u8` in [`BondEscrow::state`].
#[repr(u8)]
pub enum BondState {
    Escrowed = 0,
    Provisional = 1,
    Disputed = 2,
    Finalized = 3,
}

/// The on-chain bond record — one PDA per intent digest.
#[account]
pub struct BondEscrow {
    pub agent: Pubkey,
    pub caller: Pubkey,
    pub mint: Pubkey,
    pub vault: Pubkey,
    pub authority: Pubkey,
    pub challenger: Pubkey,
    pub digest: [u8; 32],
    pub bond_amount: u64,
    pub challenger_stake: u64,
    pub min_out_raw: u64,
    pub deadline_unix: i64,
    pub dispute_window_secs: i64,
    pub window_closes_unix: i64,
    pub routing: SlashRouting,
    pub state: u8,
    pub provisional_honored: bool,
    pub final_slashed: bool,
    pub bump: u8,
}

impl BondEscrow {
    // discriminator + 6 pubkeys + digest + 3 u64 + 3 i64 + routing(4*u16) + 3 u8 + 2 bool
    pub const LEN: usize = 8 + (32 * 6) + 32 + (8 * 3) + (8 * 3) + 8 + 3 + 2;
}

/// CPI helper: move `amount` out of the vault, signed by the escrow PDA.
fn transfer_from_vault<'info>(
    ctx: &Context<Settle<'info>>,
    to: AccountInfo<'info>,
    amount: u64,
    seeds: &[&[u8]],
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    // Named binding, not `&[seeds]` inline — see the note in `settle`.
    let signer_seeds = [seeds];
    let cpi = CpiContext::new_with_signer(
        ctx.accounts.token_program.to_account_info(),
        Transfer {
            from: ctx.accounts.vault.to_account_info(),
            to,
            authority: ctx.accounts.escrow.to_account_info(),
        },
        &signer_seeds,
    );
    token::transfer(cpi, amount)
}

#[derive(Accounts)]
#[instruction(digest: [u8; 32])]
pub struct EscrowBond<'info> {
    #[account(mut)]
    pub agent: Signer<'info>,

    /// The agent's funding token account (source of the bond).
    #[account(mut, constraint = agent_token.mint == mint.key() @ EscrowError::MintMismatch)]
    pub agent_token: Account<'info, TokenAccount>,

    pub mint: Account<'info, Mint>,

    /// The escrow record PDA, seeded by the intent digest.
    #[account(
        init,
        payer = agent,
        space = BondEscrow::LEN,
        seeds = [b"escrow", digest.as_ref()],
        bump,
    )]
    pub escrow: Account<'info, BondEscrow>,

    /// Program-owned vault holding the bond; the escrow PDA is its authority.
    #[account(
        init,
        payer = agent,
        seeds = [b"vault", digest.as_ref()],
        bump,
        token::mint = mint,
        token::authority = escrow,
    )]
    pub vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

impl<'info> EscrowBond<'info> {
    fn deposit_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            Transfer {
                from: self.agent_token.to_account_info(),
                to: self.vault.to_account_info(),
                authority: self.agent.to_account_info(),
            },
        )
    }
}

/// State-only transitions gated to the recorded `authority`.
#[derive(Accounts)]
pub struct AuthorityOnly<'info> {
    #[account(mut, has_one = authority @ EscrowError::Unauthorized)]
    pub escrow: Account<'info, BondEscrow>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct FileChallenge<'info> {
    #[account(mut, has_one = vault @ EscrowError::VaultMismatch)]
    pub escrow: Account<'info, BondEscrow>,

    #[account(mut)]
    pub challenger: Signer<'info>,

    #[account(mut, constraint = challenger_token.mint == escrow.mint @ EscrowError::MintMismatch)]
    pub challenger_token: Account<'info, TokenAccount>,

    #[account(mut)]
    pub vault: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

impl<'info> FileChallenge<'info> {
    fn stake_ctx(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        CpiContext::new(
            self.token_program.to_account_info(),
            Transfer {
                from: self.challenger_token.to_account_info(),
                to: self.vault.to_account_info(),
                authority: self.challenger.to_account_info(),
            },
        )
    }
}

/// Permissionless timeout finalization — no authority, only the clock.
#[derive(Accounts)]
pub struct FinalizeTimeout<'info> {
    #[account(mut)]
    pub escrow: Account<'info, BondEscrow>,
}

/// Disburse and close. All beneficiary token accounts are required so the four-way
/// split (and the honored return) can settle in one instruction. The escrow PDA
/// signs the vault transfers.
#[derive(Accounts)]
pub struct Settle<'info> {
    #[account(
        mut,
        has_one = vault @ EscrowError::VaultMismatch,
        has_one = agent @ EscrowError::Unauthorized,
        close = agent,
        seeds = [b"escrow", escrow.digest.as_ref()],
        bump = escrow.bump,
    )]
    pub escrow: Account<'info, BondEscrow>,

    #[account(mut)]
    pub vault: Account<'info, TokenAccount>,

    /// CHECK: rent destination on close; validated by `has_one = agent`.
    #[account(mut)]
    pub agent: AccountInfo<'info>,

    #[account(mut, constraint = agent_token.mint == escrow.mint @ EscrowError::MintMismatch)]
    pub agent_token: Account<'info, TokenAccount>,
    #[account(mut, constraint = caller_token.mint == escrow.mint @ EscrowError::MintMismatch)]
    pub caller_token: Account<'info, TokenAccount>,
    #[account(mut, constraint = challenger_token.mint == escrow.mint @ EscrowError::MintMismatch)]
    pub challenger_token: Account<'info, TokenAccount>,
    #[account(mut, constraint = insurance_token.mint == escrow.mint @ EscrowError::MintMismatch)]
    pub insurance_token: Account<'info, TokenAccount>,
    #[account(mut, constraint = lineage_token.mint == escrow.mint @ EscrowError::MintMismatch)]
    pub lineage_token: Account<'info, TokenAccount>,

    pub token_program: Program<'info, Token>,
}

#[error_code]
pub enum EscrowError {
    #[msg("Slash routing bps must sum to 10_000")]
    InvalidRouting,
    #[msg("Bond amount must be greater than zero")]
    ZeroBond,
    #[msg("Challenge stake must be greater than zero")]
    ZeroStake,
    #[msg("Instruction not valid from the bond's current state")]
    BadState,
    #[msg("Dispute window has closed")]
    WindowClosed,
    #[msg("Dispute window is still open")]
    WindowOpen,
    #[msg("Bond deadline has not passed")]
    DeadlineNotPassed,
    #[msg("Provisional bond is already slashed; nothing to challenge")]
    NothingToChallenge,
    #[msg("Signer is not the recorded authority")]
    Unauthorized,
    #[msg("Vault account does not match the escrow record")]
    VaultMismatch,
    #[msg("Token account mint does not match the bond mint")]
    MintMismatch,
}
