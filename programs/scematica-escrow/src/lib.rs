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
        resolve_grace_secs: i64,
        routing: SlashRouting,
        authority: Pubkey,
        caller: Pubkey,
        insurance: Pubkey,
        lineage: Pubkey,
    ) -> Result<()> {
        require!(routing.is_valid(), EscrowError::InvalidRouting);
        require!(bond_amount > 0, EscrowError::ZeroBond);

        // **Every slice that can be paid must name the party it is paid to.**
        //
        // `settle` binds each destination token account to the owner recorded here (see
        // `struct Settle`). A slice with a non-zero share and no recorded owner would be
        // the one destination left bound by mint alone, which is exactly the hole that
        // binding closes — so it is refused at the only point where it can still be
        // refused cheaply. A zero share may leave its party unset: nothing flows there,
        // and requiring an address for a slice worth nothing would make the common
        // two-way split unconstructible.
        require!(caller != Pubkey::default(), EscrowError::UnnamedDestination);

        // **Every state must have a way out that does not require a particular person.**
        //
        // A bond nobody can settle is indistinguishable from a bond that was taken. Two
        // sentinels used to produce exactly that. `deadline_unix == 0` was accepted as
        // "no deadline", which left an `Escrowed` bond finalizable only by the authority
        // calling `mark_provisional` — so an authority that stops answering locks the
        // bond, the stake and the vault rent forever. And a `Disputed` bond could only
        // ever leave through `resolve`, which is authority-only, with no timeout at all.
        //
        // Both now need a positive bound, set here where it is still free to insist on.
        require!(deadline_unix > 0, EscrowError::UnboundedDeadline);
        require!(resolve_grace_secs > 0, EscrowError::UnboundedDeadline);
        require!(
            routing.to_insurance_bps == 0 || insurance != Pubkey::default(),
            EscrowError::UnnamedDestination
        );
        require!(
            routing.to_lineage_bps == 0 || lineage != Pubkey::default(),
            EscrowError::UnnamedDestination
        );

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
        e.resolve_grace_secs = resolve_grace_secs;
        e.window_closes_unix = 0;
        e.routing = routing;
        e.state = BondState::Escrowed as u8;
        e.provisional_honored = false;
        e.final_slashed = false;
        e.challenger = Pubkey::default();
        e.challenger_stake = 0;
        e.authority = authority;
        e.insurance = insurance;
        e.lineage = lineage;
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
                // **The unchallenged provisional outcome is what becomes final.**
                //
                // This used to write `false` unconditionally — an optimistic honor that
                // never read `provisional_honored`. The authority could mark a fill
                // dishonored and the bond still finalized honored once the window
                // elapsed, and nobody could intervene: `file_challenge` refuses a
                // provisional dishonor (`NothingToChallenge`) because there is nothing to
                // dispute. The result was an inversion of the incentive the bond exists
                // to create — an agent that delivered a bad fill kept its bond, while one
                // that delivered nothing was slashed by the deadline arm below. Doing
                // something bad was strictly better than doing nothing.
                //
                // The off-chain twin already had it right: `SettlementMachine::finalize`
                // carries the provisional `outcome` into `Finalized`.
                e.final_slashed = !e.provisional_honored;
            }
            s if s == BondState::Escrowed as u8 => {
                require!(
                    e.deadline_unix != 0 && now >= e.deadline_unix,
                    EscrowError::DeadlineNotPassed
                );
                e.final_slashed = true; // failure to deliver
            }
            s if s == BondState::Disputed as u8 => {
                // **An unanswered dispute resolves against the agent.**
                //
                // `resolve` is authority-only and there was no other exit, so a vanished
                // authority stranded the bond permanently — a liveness failure rather
                // than theft, but the money is just as unreachable either way.
                //
                // Slashing is the conservative default, and the choice matters: the
                // alternative is that an agent whose fill was challenged gets paid by the
                // authority going quiet, which makes silence something worth arranging.
                // A challenger who was wrong still recovers its stake at `settle`.
                require!(
                    now >= e.window_closes_unix.saturating_add(e.resolve_grace_secs),
                    EscrowError::WindowOpen
                );
                e.final_slashed = true;
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
        let seeds: &[&[u8]] = &[b"escrow", e.digest.as_ref(), e.agent.as_ref(), &[e.bump]];

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
    /// Destination owner for the insurance slice of a slash. `Pubkey::default()` only
    /// when `routing.to_insurance_bps == 0` — enforced at `escrow`.
    pub insurance: Pubkey,
    /// Destination owner for the lineage slice of a slash. Same rule as `insurance`.
    pub lineage: Pubkey,
    pub digest: [u8; 32],
    pub bond_amount: u64,
    pub challenger_stake: u64,
    pub min_out_raw: u64,
    pub deadline_unix: i64,
    pub dispute_window_secs: i64,
    /// How long after the dispute window closes the recorded `authority` has to
    /// `resolve` before anyone may finalize the dispute by timeout. See
    /// `finalize_timeout`'s `Disputed` arm.
    pub resolve_grace_secs: i64,
    pub window_closes_unix: i64,
    pub routing: SlashRouting,
    pub state: u8,
    pub provisional_honored: bool,
    pub final_slashed: bool,
    pub bump: u8,
}

impl BondEscrow {
    /// Account size.
    ///
    /// Written a term per field group so it can be checked against the struct by reading
    /// down it. The previous version said "3 u8 + 2 bool" and allocated five bytes for
    /// what is four single-byte fields (`state`, two `bool`s, `bump`) — harmless in
    /// itself, since over-allocation only costs rent, but the next person to add a field
    /// would have trusted the comment.
    pub const LEN: usize = 8                 // discriminator
        + (32 * 8)                           // agent, caller, mint, vault, authority,
                                             //   challenger, insurance, lineage
        + 32                                 // digest
        + (8 * 3)                            // bond_amount, challenger_stake, min_out_raw
        + (8 * 4)                            // deadline, dispute_window, resolve_grace,
                                             //   window_closes
        + (2 * 4)                            // routing: four u16
        + 4; // state, provisional_honored, final_slashed, bump
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
        // ## The agent is part of the address
        //
        // Seeded by the digest alone, the record was squattable: anyone who learned an
        // intent digest before the agent submitted could open the escrow first with a
        // one-unit bond and an `authority` they controlled. The real agent's `init` then
        // failed on an address already in use, and any party reading "there is an escrow
        // for this digest" without also reading `agent`, `authority`, `bond_amount` and
        // `routing` was looking at the squatter's record.
        //
        // Including the agent means records cannot collide across agents at all, so the
        // race disappears rather than being won. It does not remove the obligation on a
        // consumer to validate the record's fields rather than its existence.
        seeds = [b"escrow", digest.as_ref(), agent.key().as_ref()],
        bump,
    )]
    pub escrow: Account<'info, BondEscrow>,

    /// Program-owned vault holding the bond; the escrow PDA is its authority.
    #[account(
        init,
        payer = agent,
        seeds = [b"vault", digest.as_ref(), agent.key().as_ref()],
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
        seeds = [b"escrow", escrow.digest.as_ref(), agent.key().as_ref()],
        bump = escrow.bump,
    )]
    pub escrow: Account<'info, BondEscrow>,

    #[account(mut)]
    pub vault: Account<'info, TokenAccount>,

    /// CHECK: rent destination on close; validated by `has_one = agent`.
    #[account(mut)]
    pub agent: AccountInfo<'info>,

    // ## Every destination is bound to the party the record names
    //
    // `settle` is permissionless, and that is deliberate: finalization is a public fact
    // and disbursing against it should not need a privileged caller. What made that
    // dangerous was the account validation — each destination carried a mint constraint
    // and nothing else, so any observer could call `settle` on a finalized bond, pass a
    // token account they owned of the right mint, and receive the payout. On the honored
    // branch that is `bond + challenger_stake`: the entire escrow.
    //
    // `has_one = agent` does **not** cover this. It constrains the `AccountInfo` that
    // receives the closed account's rent, not the token account that receives the money.
    //
    // The sibling program already had the right shape — `scematica-vault`'s `Withdraw`
    // constrains `depositor_token.owner == depositor.key()` — so this is that constraint,
    // applied to all five slices.
    #[account(
        mut,
        constraint = agent_token.mint == escrow.mint @ EscrowError::MintMismatch,
        constraint = agent_token.owner == escrow.agent @ EscrowError::DestinationMismatch,
    )]
    pub agent_token: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = caller_token.mint == escrow.mint @ EscrowError::MintMismatch,
        constraint = caller_token.owner == escrow.caller @ EscrowError::DestinationMismatch,
    )]
    pub caller_token: Account<'info, TokenAccount>,
    /// The challenger's account. Bound only when there **is** a challenger: with none
    /// recorded, `settle` rolls the challenger slice to the caller and pays nothing here,
    /// and requiring an account owned by `Pubkey::default()` would make the uncontested
    /// slash unsettleable. The binding therefore holds exactly where money can move.
    #[account(
        mut,
        constraint = challenger_token.mint == escrow.mint @ EscrowError::MintMismatch,
        constraint = escrow.challenger == Pubkey::default()
            || challenger_token.owner == escrow.challenger @ EscrowError::DestinationMismatch,
    )]
    pub challenger_token: Account<'info, TokenAccount>,
    /// Same rule as the challenger: unset is permitted only because `escrow` refuses to
    /// record an unset party against a non-zero share.
    #[account(
        mut,
        constraint = insurance_token.mint == escrow.mint @ EscrowError::MintMismatch,
        constraint = escrow.insurance == Pubkey::default()
            || insurance_token.owner == escrow.insurance @ EscrowError::DestinationMismatch,
    )]
    pub insurance_token: Account<'info, TokenAccount>,
    #[account(
        mut,
        constraint = lineage_token.mint == escrow.mint @ EscrowError::MintMismatch,
        constraint = escrow.lineage == Pubkey::default()
            || lineage_token.owner == escrow.lineage @ EscrowError::DestinationMismatch,
    )]
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
    #[msg("Payout destination is not owned by the party recorded in the escrow")]
    DestinationMismatch,
    #[msg("A slice with a non-zero share must name the party it pays")]
    UnnamedDestination,
    #[msg("Deadline and dispute-resolution grace must both be positive")]
    UnboundedDeadline,
}
