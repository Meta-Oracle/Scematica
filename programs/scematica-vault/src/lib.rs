use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    self, Mint, TokenAccount, TokenInterface, TransferChecked,
};

// The real program ID. Its keypair lives at target/deploy/scemadex_vault-keypair.json
// (gitignored via `programs/*/target/`) and is the deploy + upgrade authority until
// `set-upgrade-authority --final` is run. See DEPLOY.md §2 and §5.
declare_id!("A7h6khtKFJEu46By7C4hREdMQKkgvnuBCbVyusZRu4YW");

/// # The ScemaDEX Escrow Market vault
///
/// Time-locked, non-custodial backing for **any** SPL token. A depositor locks some
/// amount of a reserve asset (wBTC / wETH / any mint) — optionally alongside the token
/// itself — for a fixed period. Anyone can read the vault and see exactly how much
/// reserve stands behind how much token, and for how long.
///
/// The product is not "we hold your money". It is a **costly, irreversible, publicly
/// verifiable signal** in a market where essentially nothing is backed. A team that
/// locks real wBTC behind its token has done something a rug cannot cheaply fake.
///
/// ## The custody guarantee, and how to check it yourself
///
/// The whole value of this program is a negative claim: **nobody — including its
/// deployer — can take these funds.** That claim rests on three properties, all of
/// which are verifiable on-chain by a stranger. Losing any one of them voids it:
///
/// 1. **No privileged role exists.** There is no `authority` field on any account in
///    this file, and no instruction accepts an admin signer. Compare
///    `scemadex-escrow`, which deliberately *does* have an `authority` (a facilitator
///    that adjudicates disputes) — that design is correct for a performance bond and
///    completely wrong here. Grep this file for `authority`: the only hits are
///    `token::authority`, which names the vault PDA itself.
///
/// 2. **The vaults are PDA-owned.** `token::authority = vault`, where `vault` is a
///    program-derived address. No private key exists for it, so the only way funds
///    move is an instruction in this program signing via `invoke_signed` — and the
///    only instruction that moves funds *out* is [`withdraw`], which pays the
///    recorded depositor and nobody else.
///
/// 3. **The program is not upgradeable.** This is the one that is not visible in the
///    source, and it is the one most projects get wrong. An upgradeable program can be
///    redeployed with a drain instruction by whoever holds the upgrade authority — so
///    a PDA vault under an upgradeable program is *fully custodial*, no matter how
///    this file reads. It must be finalized after deploy:
///
///    ```text
///    solana program set-upgrade-authority <PROGRAM_ID> --final
///    solana program show <PROGRAM_ID>        # "Authority: none"
///    ```
///
///    Until that shows `none`, requirement 5 is not met. See `DEPLOY.md`.
///
/// ## What this program deliberately does not do
///
/// - **No oracle, no price.** The vault reports raw amounts: X of token locked, Y of
///   reserve locked. Any USD figure or "percent backed" is computed off-chain by the
///   reader with their own price source. This removes oracle manipulation from the
///   attack surface entirely, and it keeps the on-chain record to things that cannot
///   be argued with.
/// - **No redemption.** Nobody can burn the token to claim reserve. Redemption is the
///   regulated shape and the run vector; it is a separate, later decision made with
///   real reserves and real legal advice. Adding it must never mean upgrading *this*
///   program — it means a new one that users opt into.
/// - **No lending.** Same reasoning, more sharply: lending rules would be frozen
///   forever by the immutability above, and lending is where nearly all DeFi exploits
///   live. It belongs in a separate v2 program that depositors migrate into by choice,
///   so this vault's guarantee is never weakened by it.
/// - **No early exit, for anyone, ever.** A lock that can be cut short is not a lock,
///   and the signal it sends is worth exactly nothing.
#[program]
pub mod scemadex_vault {
    use super::*;

    /// Open the canonical vault for a `(token_mint, backing_mint)` pair.
    ///
    /// Permissionless by design: gating this on an admin would create precisely the
    /// privileged role the program exists to not have. The seeds make the vault
    /// deterministic and unique per pair, so there is exactly one canonical vault to
    /// read for any given pairing and no ambiguity about which one is "the real one".
    pub fn initialize_vault(ctx: Context<InitializeVault>) -> Result<()> {
        let v = &mut ctx.accounts.vault;
        v.token_mint = ctx.accounts.token_mint.key();
        v.backing_mint = ctx.accounts.backing_mint.key();
        v.token_vault = ctx.accounts.token_vault.key();
        v.backing_vault = ctx.accounts.backing_vault.key();
        v.total_token_locked = 0;
        v.total_backing_locked = 0;
        v.positions_open = 0;
        v.positions_lifetime = 0;
        v.bump = ctx.bumps.vault;

        msg!(
            "initialize_vault: token={} backing={}",
            v.token_mint,
            v.backing_mint
        );
        Ok(())
    }

    /// Lock a position for `lock_secs`, transferring the reserve (and optionally the
    /// token) into the vault.
    ///
    /// `backing_amount` must be non-zero but `token_amount` may be zero, and the
    /// asymmetry is deliberate: a pure-reserve deposit is a supporter strengthening
    /// the backing, which can only ever help. The reverse — token with no reserve —
    /// would let anyone dilute a vault's headline ratio for free, so it is rejected.
    ///
    /// `nonce` lets one depositor hold many independent positions in the same vault
    /// with different unlock dates.
    pub fn deposit(
        ctx: Context<Deposit>,
        nonce: u64,
        token_amount: u64,
        backing_amount: u64,
        lock_secs: i64,
    ) -> Result<()> {
        require!(backing_amount > 0, VaultError::ZeroBacking);
        require!(
            (MIN_LOCK_SECS..=MAX_LOCK_SECS).contains(&lock_secs),
            VaultError::LockOutOfRange
        );

        let now = Clock::get()?.unix_timestamp;
        let unlock_unix = now.checked_add(lock_secs).ok_or(VaultError::MathOverflow)?;

        // Credit what *arrived*, not what was asked for.
        //
        // Token-2022 mints may carry a transfer fee, so the vault can receive less than
        // `backing_amount`. Recording the requested figure would book reserve that is
        // not there: the vault would be short by the fee on every deposit, the published
        // backing total would overstate the truth, and eventually some depositor's
        // `withdraw` would fail on a vault that looked solvent. Measuring the balance
        // delta makes the record true by construction for fee'd and fee-less mints
        // alike — and it is why a fee-bearing mint simply results in a smaller position
        // rather than a broken vault.
        let backing_before = ctx.accounts.backing_vault.amount;
        token_interface::transfer_checked(
            ctx.accounts.backing_deposit_ctx(),
            backing_amount,
            ctx.accounts.backing_mint.decimals,
        )?;
        ctx.accounts.backing_vault.reload()?;
        let backing_received = ctx
            .accounts
            .backing_vault
            .amount
            .checked_sub(backing_before)
            .ok_or(VaultError::AccountingUnderflow)?;
        require!(backing_received > 0, VaultError::ZeroBacking);

        let token_received = if token_amount > 0 {
            let token_before = ctx.accounts.token_vault.amount;
            token_interface::transfer_checked(
                ctx.accounts.token_deposit_ctx(),
                token_amount,
                ctx.accounts.token_mint.decimals,
            )?;
            ctx.accounts.token_vault.reload()?;
            ctx.accounts
                .token_vault
                .amount
                .checked_sub(token_before)
                .ok_or(VaultError::AccountingUnderflow)?
        } else {
            0
        };

        let v = &mut ctx.accounts.vault;
        v.total_token_locked = v
            .total_token_locked
            .checked_add(token_received)
            .ok_or(VaultError::MathOverflow)?;
        v.total_backing_locked = v
            .total_backing_locked
            .checked_add(backing_received)
            .ok_or(VaultError::MathOverflow)?;
        v.positions_open = v.positions_open.checked_add(1).ok_or(VaultError::MathOverflow)?;
        v.positions_lifetime = v
            .positions_lifetime
            .checked_add(1)
            .ok_or(VaultError::MathOverflow)?;

        let p = &mut ctx.accounts.position;
        p.vault = v.key();
        p.depositor = ctx.accounts.depositor.key();
        p.token_amount = token_received;
        p.backing_amount = backing_received;
        p.created_unix = now;
        p.unlock_unix = unlock_unix;
        p.nonce = nonce;
        p.bump = ctx.bumps.position;

        msg!(
            "deposit: depositor={} token={} backing={} unlock={}",
            p.depositor,
            token_received,
            backing_received,
            unlock_unix
        );
        Ok(())
    }

    /// Push a position's unlock further out. Depositor-only, and **strictly
    /// increasing**.
    ///
    /// The monotonicity is the whole point: a depositor may always strengthen a
    /// commitment and may never weaken one. Allowing this to move the date backwards —
    /// even by the depositor's own signature — would turn every lock into a
    /// suggestion, and an observer could no longer trust a published unlock date.
    pub fn extend_lock(ctx: Context<ExtendLock>, new_unlock_unix: i64) -> Result<()> {
        let p = &mut ctx.accounts.position;
        require!(new_unlock_unix > p.unlock_unix, VaultError::LockNotExtended);
        let now = Clock::get()?.unix_timestamp;
        require!(
            new_unlock_unix <= now.saturating_add(MAX_LOCK_SECS),
            VaultError::LockOutOfRange
        );
        let previous = p.unlock_unix;
        p.unlock_unix = new_unlock_unix;
        msg!("extend_lock: {} -> {}", previous, new_unlock_unix);
        Ok(())
    }

    /// Return a matured position to the depositor and close it.
    ///
    /// The only instruction in this program that moves funds out of a vault. It pays
    /// `position.depositor` and no one else, transfers exactly the amounts recorded at
    /// deposit (never the vault's balance — one position must not be able to reach
    /// another's funds), and closes the position account so the withdrawal cannot be
    /// replayed.
    pub fn withdraw(ctx: Context<Withdraw>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let (token_amount, backing_amount) = {
            let p = &ctx.accounts.position;
            require!(now >= p.unlock_unix, VaultError::StillLocked);
            (p.token_amount, p.backing_amount)
        };

        let token_mint = ctx.accounts.vault.token_mint;
        let backing_mint = ctx.accounts.vault.backing_mint;
        let bump = ctx.accounts.vault.bump;
        let seeds: &[&[u8]] = &[
            b"vault",
            token_mint.as_ref(),
            backing_mint.as_ref(),
            &[bump],
        ];

        if backing_amount > 0 {
            transfer_from_vault(
                &ctx.accounts.backing_token_program,
                &ctx.accounts.backing_vault,
                &ctx.accounts.backing_mint,
                &ctx.accounts.depositor_backing,
                &ctx.accounts.vault,
                backing_amount,
                seeds,
            )?;
        }
        if token_amount > 0 {
            transfer_from_vault(
                &ctx.accounts.token_token_program,
                &ctx.accounts.token_vault,
                &ctx.accounts.token_mint,
                &ctx.accounts.depositor_token,
                &ctx.accounts.vault,
                token_amount,
                seeds,
            )?;
        }

        let v = &mut ctx.accounts.vault;
        // `saturating_sub` would silently paper over an accounting bug here, so these
        // are checked: if the totals ever disagree with the positions, the withdrawal
        // must fail loudly rather than quietly report a wrong backing figure.
        v.total_token_locked = v
            .total_token_locked
            .checked_sub(token_amount)
            .ok_or(VaultError::AccountingUnderflow)?;
        v.total_backing_locked = v
            .total_backing_locked
            .checked_sub(backing_amount)
            .ok_or(VaultError::AccountingUnderflow)?;
        v.positions_open = v
            .positions_open
            .checked_sub(1)
            .ok_or(VaultError::AccountingUnderflow)?;

        msg!(
            "withdraw: depositor={} token={} backing={}",
            ctx.accounts.depositor.key(),
            token_amount,
            backing_amount
        );
        Ok(())
    }
}

/// Shortest permitted lock: 7 days.
///
/// A floor exists so "backed" cannot mean a position opened and closed inside a block
/// to produce a screenshot. Seven days is long enough that the commitment has real
/// opportunity cost and short enough not to scare off a first deposit.
pub const MIN_LOCK_SECS: i64 = 7 * 24 * 60 * 60;

/// Longest permitted lock: ~10 years. Bounds the arithmetic and stops a fat-fingered
/// `lock_secs` from creating a position nobody alive can withdraw.
pub const MAX_LOCK_SECS: i64 = 10 * 365 * 24 * 60 * 60;

/// One canonical vault per `(token_mint, backing_mint)` pair.
///
/// Note what is absent: no authority, no admin, no pause flag, no fee recipient.
/// Adding any of them reintroduces the trusted party this program exists to remove.
#[account]
pub struct Vault {
    pub token_mint: Pubkey,
    pub backing_mint: Pubkey,
    pub token_vault: Pubkey,
    pub backing_vault: Pubkey,
    /// Sum of live positions' token legs.
    ///
    /// The token vault's balance is `>=` this, never `<`. Anyone may transfer SPL
    /// tokens into any account, so a stranger can donate into the vault and push the
    /// balance above the sum of live positions. Such donations are **permanently
    /// stuck**: [`withdraw`] moves only amounts recorded on a [`Position`], and a
    /// sweeper instruction would mean reintroducing a privileged role. Funds nobody
    /// can move are a better outcome than an admin who can.
    ///
    /// A balance *below* this total is the alarming direction — it means the
    /// accounting and the tokens disagree.
    pub total_token_locked: u64,
    /// Sum of live positions' reserve legs. Same `>=` relationship as
    /// [`Self::total_token_locked`].
    pub total_backing_locked: u64,
    pub positions_open: u64,
    pub positions_lifetime: u64,
    pub bump: u8,
}

impl Vault {
    // discriminator + 4 pubkeys + 4 u64 + bump
    pub const LEN: usize = 8 + (32 * 4) + (8 * 4) + 1;
}

/// One time-locked deposit. Closed and rent-refunded on withdraw.
#[account]
pub struct Position {
    pub vault: Pubkey,
    pub depositor: Pubkey,
    pub token_amount: u64,
    pub backing_amount: u64,
    pub created_unix: i64,
    pub unlock_unix: i64,
    pub nonce: u64,
    pub bump: u8,
}

impl Position {
    // discriminator + 2 pubkeys + 3 u64 + 2 i64 + bump
    pub const LEN: usize = 8 + (32 * 2) + (8 * 3) + (8 * 2) + 1;
}

/// CPI helper: move `amount` out of a vault token account, signed by the vault PDA.
///
/// `transfer_checked` rather than `transfer`: Token-2022 requires it, and it validates
/// the mint and decimals against the accounts, which `transfer` does not.
#[allow(clippy::too_many_arguments)]
fn transfer_from_vault<'info>(
    token_program: &Interface<'info, TokenInterface>,
    from: &InterfaceAccount<'info, TokenAccount>,
    mint: &InterfaceAccount<'info, Mint>,
    to: &InterfaceAccount<'info, TokenAccount>,
    vault: &Account<'info, Vault>,
    amount: u64,
    seeds: &[&[u8]],
) -> Result<()> {
    // Bound to a `let` rather than passed as `&[seeds]` inline: the latter is a
    // temporary that is dropped at the end of the statement while the CpiContext still
    // borrows it.
    let signer_seeds: [&[&[u8]]; 1] = [seeds];
    let cpi = CpiContext::new_with_signer(
        token_program.to_account_info(),
        TransferChecked {
            from: from.to_account_info(),
            mint: mint.to_account_info(),
            to: to.to_account_info(),
            authority: vault.to_account_info(),
        },
        &signer_seeds,
    );
    token_interface::transfer_checked(cpi, amount, mint.decimals)
}

/// Every account here is `Box`ed, and that is a correctness requirement rather than a
/// style choice. Anchor generates one `try_accounts` frame for the whole struct, and
/// three `init` accounts plus two `Mint` deserializations put it at 5072 bytes against
/// SBF's hard 4096-byte stack limit. `cargo-build-sbf` reports that overflow but still
/// exits 0 and emits a `.so`, so an unboxed build deploys cleanly and then fails at
/// runtime with a stack access violation — meaning no vault could ever be created.
/// Boxing moves the payloads to the heap. Deref makes it invisible to the handlers.
#[derive(Accounts)]
pub struct InitializeVault<'info> {
    /// Pays rent. Gains no rights whatsoever by doing so — this is not an owner.
    #[account(mut)]
    pub payer: Signer<'info>,

    /// Backing a mint with itself is not a product, it is a misreading waiting to
    /// happen — the published ratio would be meaningless. Rejected outright.
    #[account(constraint = token_mint.key() != backing_mint.key() @ VaultError::SameMint)]
    pub token_mint: Box<InterfaceAccount<'info, Mint>>,
    pub backing_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = payer,
        space = Vault::LEN,
        seeds = [b"vault", token_mint.key().as_ref(), backing_mint.key().as_ref()],
        bump,
    )]
    pub vault: Box<Account<'info, Vault>>,

    #[account(
        init,
        payer = payer,
        seeds = [b"token_vault", vault.key().as_ref()],
        bump,
        token::mint = token_mint,
        token::authority = vault,
        token::token_program = token_token_program,
    )]
    pub token_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        init,
        payer = payer,
        seeds = [b"backing_vault", vault.key().as_ref()],
        bump,
        token::mint = backing_mint,
        token::authority = vault,
        token::token_program = backing_token_program,
    )]
    pub backing_vault: Box<InterfaceAccount<'info, TokenAccount>>,

    /// One token program **per leg**, not one for the pair.
    ///
    /// These are separate accounts because the two mints need not live on the same token
    /// program, and the interesting cases are exactly the mixed ones: a Token-2022 memecoin
    /// (SCEMA is one) backed by legacy-SPL wBTC, wETH or wSOL. A single shared
    /// `token_program` field made every such pair unconstructible — which is to say it made
    /// the product's headline use case impossible — while adding no safety, since a mint's
    /// owning program is not a matter of opinion.
    ///
    /// Passing the wrong program for a leg cannot create a bad vault: the `init` CPI runs
    /// against the program named here, and `initialize_account3` fails outright when the
    /// mint is not owned by it. Both fields may name the same program, and do whenever the
    /// pair happens to be same-program — a duplicate account key is ordinary.
    pub token_token_program: Interface<'info, TokenInterface>,
    pub backing_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    // No `rent: Sysvar<'info, Rent>`. Anchor's `init` reads rent via `Rent::get()`, so
    // the account is dead weight — and on this struct it is weight the 4096-byte SBF
    // stack frame cannot afford. See the note above.
}

#[derive(Accounts)]
#[instruction(nonce: u64)]
pub struct Deposit<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,

    #[account(
        mut,
        seeds = [b"vault", vault.token_mint.as_ref(), vault.backing_mint.as_ref()],
        bump = vault.bump,
        has_one = token_vault @ VaultError::VaultMismatch,
        has_one = backing_vault @ VaultError::VaultMismatch,
    )]
    pub vault: Account<'info, Vault>,

    #[account(
        init,
        payer = depositor,
        space = Position::LEN,
        seeds = [b"position", vault.key().as_ref(), depositor.key().as_ref(), &nonce.to_le_bytes()],
        bump,
    )]
    pub position: Account<'info, Position>,

    #[account(mut)]
    pub token_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(mut)]
    pub backing_vault: InterfaceAccount<'info, TokenAccount>,

    #[account(constraint = token_mint.key() == vault.token_mint @ VaultError::MintMismatch)]
    pub token_mint: InterfaceAccount<'info, Mint>,
    #[account(constraint = backing_mint.key() == vault.backing_mint @ VaultError::MintMismatch)]
    pub backing_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        constraint = depositor_token.mint == vault.token_mint @ VaultError::MintMismatch,
    )]
    pub depositor_token: InterfaceAccount<'info, TokenAccount>,
    #[account(
        mut,
        constraint = depositor_backing.mint == vault.backing_mint @ VaultError::MintMismatch,
    )]
    pub depositor_backing: InterfaceAccount<'info, TokenAccount>,

    /// Per-leg token programs — see [`InitializeVault`]. Each leg's transfer is a CPI to
    /// the program that owns that leg's mint, so a mixed-program pair moves both halves
    /// correctly and a mismatched program simply fails the transfer.
    pub token_token_program: Interface<'info, TokenInterface>,
    pub backing_token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
    pub rent: Sysvar<'info, Rent>,
}

impl<'info> Deposit<'info> {
    fn token_deposit_ctx(&self) -> CpiContext<'_, '_, '_, 'info, TransferChecked<'info>> {
        CpiContext::new(
            self.token_token_program.to_account_info(),
            TransferChecked {
                from: self.depositor_token.to_account_info(),
                mint: self.token_mint.to_account_info(),
                to: self.token_vault.to_account_info(),
                authority: self.depositor.to_account_info(),
            },
        )
    }

    fn backing_deposit_ctx(&self) -> CpiContext<'_, '_, '_, 'info, TransferChecked<'info>> {
        CpiContext::new(
            self.backing_token_program.to_account_info(),
            TransferChecked {
                from: self.depositor_backing.to_account_info(),
                mint: self.backing_mint.to_account_info(),
                to: self.backing_vault.to_account_info(),
                authority: self.depositor.to_account_info(),
            },
        )
    }
}

#[derive(Accounts)]
pub struct ExtendLock<'info> {
    pub depositor: Signer<'info>,

    #[account(
        mut,
        has_one = depositor @ VaultError::NotDepositor,
    )]
    pub position: Account<'info, Position>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub depositor: Signer<'info>,

    #[account(
        mut,
        seeds = [b"vault", vault.token_mint.as_ref(), vault.backing_mint.as_ref()],
        bump = vault.bump,
        has_one = token_vault @ VaultError::VaultMismatch,
        has_one = backing_vault @ VaultError::VaultMismatch,
    )]
    pub vault: Account<'info, Vault>,

    /// Closed on success, rent back to the depositor. Closing is what makes the
    /// withdrawal non-replayable.
    #[account(
        mut,
        close = depositor,
        has_one = depositor @ VaultError::NotDepositor,
        has_one = vault @ VaultError::VaultMismatch,
    )]
    pub position: Account<'info, Position>,

    #[account(mut)]
    pub token_vault: InterfaceAccount<'info, TokenAccount>,
    #[account(mut)]
    pub backing_vault: InterfaceAccount<'info, TokenAccount>,

    #[account(constraint = token_mint.key() == vault.token_mint @ VaultError::MintMismatch)]
    pub token_mint: InterfaceAccount<'info, Mint>,
    #[account(constraint = backing_mint.key() == vault.backing_mint @ VaultError::MintMismatch)]
    pub backing_mint: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        constraint = depositor_token.mint == vault.token_mint @ VaultError::MintMismatch,
        constraint = depositor_token.owner == depositor.key() @ VaultError::NotDepositor,
    )]
    pub depositor_token: InterfaceAccount<'info, TokenAccount>,
    #[account(
        mut,
        constraint = depositor_backing.mint == vault.backing_mint @ VaultError::MintMismatch,
        constraint = depositor_backing.owner == depositor.key() @ VaultError::NotDepositor,
    )]
    pub depositor_backing: InterfaceAccount<'info, TokenAccount>,

    /// Per-leg token programs — see [`InitializeVault`].
    pub token_token_program: Interface<'info, TokenInterface>,
    pub backing_token_program: Interface<'info, TokenInterface>,
}

#[error_code]
pub enum VaultError {
    #[msg("Backing amount must be greater than zero")]
    ZeroBacking,
    #[msg("Lock duration outside the permitted range (7 days to 10 years)")]
    LockOutOfRange,
    #[msg("Position is still locked")]
    StillLocked,
    #[msg("New unlock time must be later than the current one")]
    LockNotExtended,
    #[msg("Signer is not this position's depositor")]
    NotDepositor,
    #[msg("Vault account does not match the record")]
    VaultMismatch,
    #[msg("Token account mint does not match the vault")]
    MintMismatch,
    #[msg("Arithmetic overflow")]
    MathOverflow,
    #[msg("Vault accounting underflow — totals disagree with positions")]
    AccountingUnderflow,
    #[msg("A token cannot be backed by itself")]
    SameMint,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_bounds_are_a_week_and_a_decade() {
        assert_eq!(MIN_LOCK_SECS, 604_800);
        assert!((MIN_LOCK_SECS..=MAX_LOCK_SECS).contains(&MIN_LOCK_SECS));
        assert!((MIN_LOCK_SECS..=MAX_LOCK_SECS).contains(&MAX_LOCK_SECS));
        // A "lock" measured in minutes is the screenshot attack the floor exists for.
        assert!(!(MIN_LOCK_SECS..=MAX_LOCK_SECS).contains(&60));
        assert!(!(MIN_LOCK_SECS..=MAX_LOCK_SECS).contains(&0));
        assert!(!(MIN_LOCK_SECS..=MAX_LOCK_SECS).contains(&-1));
    }

    #[test]
    fn account_sizes_match_their_fields() {
        assert_eq!(Vault::LEN, 8 + 128 + 32 + 1);
        assert_eq!(Position::LEN, 8 + 64 + 24 + 16 + 1);
    }

    /// The unlock instant is computed by addition that must not wrap. A hostile
    /// `lock_secs` is already bounded by `MAX_LOCK_SECS`, but the clock is not, so the
    /// checked add is what actually holds under a far-future timestamp.
    #[test]
    fn unlock_computation_cannot_wrap() {
        assert_eq!(i64::MAX.checked_add(MAX_LOCK_SECS), None);
        let now: i64 = 1_786_000_000;
        assert_eq!(
            now.checked_add(MIN_LOCK_SECS),
            Some(now + 604_800),
            "ordinary case still computes"
        );
    }
}
