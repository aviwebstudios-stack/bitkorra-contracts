//! Beryl Escrow: the P2P marketplace's escrow on Solana devnet (Phase I-8).
//!
//! A seller locks SOL or the accepted SPL token (devnet: Circle's USDC) for
//! one order. The money sits in program accounts (the escrow PDA's own
//! lamports for SOL, a vault token account PDA for tokens), never with Beryl:
//!
//! - the seller can release to the buyer at any time;
//! - the buyer marks it paid before the deadline, which stops the seller
//!   from reclaiming;
//! - after the deadline, if it was never marked paid, the seller reclaims it;
//! - the buyer can cancel before paying (it goes back to the seller);
//! - after "paid", either side can open a dispute, and then only the arbiter
//!   can resolve it (to the buyer or back to the seller), or the seller can
//!   still release.
//!
//! Money only ever goes to the escrow's seller, its buyer, or (a release's
//! fee) the config's treasury. A release's fee is basis points fixed when the
//! escrow was funded: 0 by default, never above 2%. Every settlement closes
//! the escrow (and the vault) and the rent goes back to the seller. The owner
//! and the arbiter change hands in two steps.
//!
//! Security: signer checks on every party, every PDA checked against its
//! seeds and stored bump, token accounts checked for the escrow's mint and
//! their owner (classic SPL Token only), checked maths, an event for every
//! change.

use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::token::{self, CloseAccount, Mint, Token, TokenAccount, TransferChecked};

pub mod errors;
pub mod state;

use errors::EscrowError;
use state::*;

declare_id!("8wDapxboCTEKfBzNQzrzZUnspmTixKFfCuNmUuQLrcDA");

#[program]
pub mod beryl_escrow {
    use super::*;

    /// Creates the config. Only the program's upgrade authority can, and only once.
    pub fn initialize(
        ctx: Context<Initialize>,
        owner: Pubkey,
        arbiter: Pubkey,
        treasury: Pubkey,
    ) -> Result<()> {
        for k in [owner, arbiter, treasury] {
            require_keys_neq!(k, Pubkey::default(), EscrowError::InvalidKey);
        }
        let c = &mut ctx.accounts.config;
        c.owner = owner;
        c.pending_owner = None;
        c.arbiter = arbiter;
        c.pending_arbiter = None;
        c.treasury = treasury;
        c.mint = ctx.accounts.mint.key();
        c.fee_bps = 0;
        c.bump = ctx.bumps.config;
        emit!(ConfigInitialized {
            owner,
            arbiter,
            treasury,
            mint: c.mint
        });
        Ok(())
    }

    pub fn set_fee(ctx: Context<OwnerOnly>, fee_bps: u16) -> Result<()> {
        require!(fee_bps <= MAX_FEE_BPS, EscrowError::FeeTooHigh);
        let c = &mut ctx.accounts.config;
        emit!(FeeSet {
            old_bps: c.fee_bps,
            new_bps: fee_bps
        });
        c.fee_bps = fee_bps;
        Ok(())
    }

    pub fn set_treasury(ctx: Context<OwnerOnly>, treasury: Pubkey) -> Result<()> {
        require_keys_neq!(treasury, Pubkey::default(), EscrowError::InvalidKey);
        let c = &mut ctx.accounts.config;
        emit!(TreasurySet {
            old: c.treasury,
            new: treasury
        });
        c.treasury = treasury;
        Ok(())
    }

    /// Step 1 of a new owner. The default key cancels.
    pub fn propose_owner(ctx: Context<OwnerOnly>, new_owner: Pubkey) -> Result<()> {
        let c = &mut ctx.accounts.config;
        c.pending_owner = (new_owner != Pubkey::default()).then_some(new_owner);
        emit!(OwnerProposed {
            pending: c.pending_owner
        });
        Ok(())
    }

    pub fn accept_owner(ctx: Context<Accept>) -> Result<()> {
        let c = &mut ctx.accounts.config;
        let pending = c.pending_owner.ok_or(EscrowError::NothingPending)?;
        require_keys_eq!(pending, ctx.accounts.new_key.key(), EscrowError::NotPending);
        emit!(OwnerAccepted {
            old: c.owner,
            new: pending
        });
        c.owner = pending;
        c.pending_owner = None;
        Ok(())
    }

    /// Step 1 of a new arbiter (the owner proposes). The default key cancels.
    pub fn propose_arbiter(ctx: Context<OwnerOnly>, new_arbiter: Pubkey) -> Result<()> {
        let c = &mut ctx.accounts.config;
        c.pending_arbiter = (new_arbiter != Pubkey::default()).then_some(new_arbiter);
        emit!(ArbiterProposed {
            pending: c.pending_arbiter
        });
        Ok(())
    }

    /// Step 2: the proposed arbiter signs to take over.
    pub fn accept_arbiter(ctx: Context<Accept>) -> Result<()> {
        let c = &mut ctx.accounts.config;
        let pending = c.pending_arbiter.ok_or(EscrowError::NothingPending)?;
        require_keys_eq!(pending, ctx.accounts.new_key.key(), EscrowError::NotPending);
        emit!(ArbiterAccepted {
            old: c.arbiter,
            new: pending
        });
        c.arbiter = pending;
        c.pending_arbiter = None;
        Ok(())
    }

    // -- funding ------------------------------------------------------------

    pub fn fund_sol(
        ctx: Context<FundSol>,
        order_id: [u8; 32],
        buyer: Pubkey,
        amount: u64,
        deadline: i64,
    ) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        check_terms(ctx.accounts.seller.key(), buyer, amount, deadline, now)?;
        let escrow_key = ctx.accounts.escrow.key();
        fill(
            &mut ctx.accounts.escrow,
            ctx.accounts.seller.key(),
            buyer,
            order_id,
            Currency::Sol,
            Pubkey::default(),
            amount,
            ctx.accounts.config.fee_bps,
            deadline,
            now,
            ctx.bumps.escrow,
            0,
        );
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.key(),
                system_program::Transfer {
                    from: ctx.accounts.seller.to_account_info(),
                    to: ctx.accounts.escrow.to_account_info(),
                },
            ),
            amount,
        )?;
        emit_funded(&ctx.accounts.escrow, escrow_key);
        Ok(())
    }

    pub fn fund_token(
        ctx: Context<FundToken>,
        order_id: [u8; 32],
        buyer: Pubkey,
        amount: u64,
        deadline: i64,
    ) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        check_terms(ctx.accounts.seller.key(), buyer, amount, deadline, now)?;
        let escrow_key = ctx.accounts.escrow.key();
        let mint = ctx.accounts.mint.key();
        fill(
            &mut ctx.accounts.escrow,
            ctx.accounts.seller.key(),
            buyer,
            order_id,
            Currency::Token,
            mint,
            amount,
            ctx.accounts.config.fee_bps,
            deadline,
            now,
            ctx.bumps.escrow,
            ctx.bumps.vault,
        );
        token::transfer_checked(
            CpiContext::new(
                ctx.accounts.token_program.key(),
                TransferChecked {
                    from: ctx.accounts.seller_token.to_account_info(),
                    mint: ctx.accounts.mint.to_account_info(),
                    to: ctx.accounts.vault.to_account_info(),
                    authority: ctx.accounts.seller.to_account_info(),
                },
            ),
            amount,
            ctx.accounts.mint.decimals,
        )?;
        emit_funded(&ctx.accounts.escrow, escrow_key);
        Ok(())
    }

    // -- the buyer and the seller -------------------------------------------

    /// The buyer says they paid, before the deadline. The seller can no longer reclaim.
    pub fn mark_paid(ctx: Context<Party>) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let e = &mut ctx.accounts.escrow;
        require_keys_eq!(ctx.accounts.signer.key(), e.buyer, EscrowError::NotBuyer);
        require!(e.state == EscrowState::Funded, EscrowError::WrongState);
        require!(now <= e.deadline, EscrowError::DeadlinePassed);
        e.state = EscrowState::Paid;
        emit!(MarkedPaid {
            escrow: e.key(),
            order_id: e.order_id,
            at: now
        });
        Ok(())
    }

    /// After "paid", either side asks Beryl to decide.
    pub fn open_dispute(ctx: Context<Party>) -> Result<()> {
        let e = &mut ctx.accounts.escrow;
        let by = ctx.accounts.signer.key();
        require!(by == e.buyer || by == e.seller, EscrowError::NotParty);
        require!(e.state == EscrowState::Paid, EscrowError::WrongState);
        e.state = EscrowState::Disputed;
        emit!(DisputeOpened {
            escrow: e.key(),
            order_id: e.order_id,
            by,
            at: Clock::get()?.unix_timestamp
        });
        Ok(())
    }

    /// The seller releases to the buyer, at any time (also during a dispute).
    pub fn release(ctx: Context<Settle>) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.signer.key(),
            ctx.accounts.escrow.seller,
            EscrowError::NotSeller
        );
        settle(ctx.accounts, true, Outcome::Released)
    }

    /// The seller takes it back after the deadline, if the buyer never marked it paid.
    pub fn refund(ctx: Context<Settle>) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.signer.key(),
            ctx.accounts.escrow.seller,
            EscrowError::NotSeller
        );
        require!(
            ctx.accounts.escrow.state == EscrowState::Funded,
            EscrowError::WrongState
        );
        require!(
            Clock::get()?.unix_timestamp > ctx.accounts.escrow.deadline,
            EscrowError::DeadlineNotPassed
        );
        settle(ctx.accounts, false, Outcome::Refunded)
    }

    /// The buyer cancels before paying: it goes back to the seller at once.
    pub fn cancel(ctx: Context<Settle>) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.signer.key(),
            ctx.accounts.escrow.buyer,
            EscrowError::NotBuyer
        );
        require!(
            ctx.accounts.escrow.state == EscrowState::Funded,
            EscrowError::WrongState
        );
        settle(ctx.accounts, false, Outcome::Cancelled)
    }

    /// The arbiter decides a dispute: to the buyer, or back to the seller.
    pub fn resolve(ctx: Context<Settle>, to_buyer: bool) -> Result<()> {
        require_keys_eq!(
            ctx.accounts.signer.key(),
            ctx.accounts.config.arbiter,
            EscrowError::NotArbiter
        );
        require!(
            ctx.accounts.escrow.state == EscrowState::Disputed,
            EscrowError::WrongState
        );
        settle(
            ctx.accounts,
            to_buyer,
            if to_buyer {
                Outcome::ResolvedBuyer
            } else {
                Outcome::ResolvedSeller
            },
        )
    }
}

// ---------------------------------------------------------------------------
// Helpers

fn check_terms(seller: Pubkey, buyer: Pubkey, amount: u64, deadline: i64, now: i64) -> Result<()> {
    require!(amount > 0, EscrowError::ZeroAmount);
    require!(
        buyer != seller && buyer != Pubkey::default(),
        EscrowError::BadBuyer
    );
    let latest = now
        .checked_add(MAX_WINDOW_SECONDS)
        .ok_or(EscrowError::MathOverflow)?;
    require!(
        deadline > now && deadline <= latest,
        EscrowError::BadDeadline
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn fill(
    e: &mut Escrow,
    seller: Pubkey,
    buyer: Pubkey,
    order_id: [u8; 32],
    currency: Currency,
    mint: Pubkey,
    amount: u64,
    fee_bps: u16,
    deadline: i64,
    now: i64,
    bump: u8,
    vault_bump: u8,
) {
    e.seller = seller;
    e.buyer = buyer;
    e.order_id = order_id;
    e.currency = currency;
    e.mint = mint;
    e.amount = amount;
    e.fee_bps = fee_bps;
    e.deadline = deadline;
    e.state = EscrowState::Funded;
    e.funded_at = now;
    e.bump = bump;
    e.vault_bump = vault_bump;
}

fn emit_funded(e: &Escrow, escrow: Pubkey) {
    emit!(EscrowFunded {
        escrow,
        order_id: e.order_id,
        seller: e.seller,
        buyer: e.buyer,
        mint: (e.currency == Currency::Token).then_some(e.mint),
        amount: e.amount,
        fee_bps: e.fee_bps,
        deadline: e.deadline,
        at: e.funded_at,
    });
}

/// Pays out and closes. To the buyer: the amount less the fee (and the fee to the
/// treasury). Otherwise everything goes back to the seller. The escrow and the
/// vault close, and their rent goes to the seller.
fn settle<'info>(a: &Settle<'info>, to_buyer: bool, outcome: Outcome) -> Result<()> {
    let e = &a.escrow;
    let (buyer_gets, fee) = if to_buyer {
        release_split(e.amount, e.fee_bps).ok_or(EscrowError::MathOverflow)?
    } else {
        (0, 0)
    };
    let seller_gets = if to_buyer { 0 } else { e.amount };
    if fee > 0 {
        let t = a.treasury.as_ref().ok_or(EscrowError::WrongAccount)?;
        require_keys_eq!(t.key(), a.config.treasury, EscrowError::WrongAccount);
    }
    match e.currency {
        Currency::Sol => {
            // Lamports move straight out of the escrow account (the program owns it).
            let from = e.to_account_info();
            move_lamports(&from, &a.buyer.to_account_info(), buyer_gets)?;
            if fee > 0 {
                move_lamports(&from, &a.treasury.as_ref().unwrap().to_account_info(), fee)?;
            }
            // What's left (the refund and the rent) goes to the seller when the account closes.
        }
        Currency::Token => {
            let (vault, mint, program) = match (&a.vault, &a.mint, &a.token_program) {
                (Some(v), Some(m), Some(p)) => (v, m, p),
                _ => return err!(EscrowError::TokenAccountsMissing),
            };
            require_keys_eq!(mint.key(), e.mint, EscrowError::WrongMint);
            let expected_vault = Pubkey::create_program_address(
                &[VAULT_SEED, e.key().as_ref(), &[e.vault_bump]],
                &crate::ID,
            )
            .map_err(|_| error!(EscrowError::WrongAccount))?;
            require_keys_eq!(vault.key(), expected_vault, EscrowError::WrongAccount);
            let seeds: &[&[u8]] = &[
                ESCROW_SEED,
                e.seller.as_ref(),
                e.order_id.as_ref(),
                &[e.bump],
            ];
            let signer: &[&[&[u8]]] = &[seeds];
            let auth = e.to_account_info();
            pay_token(
                program,
                vault,
                mint,
                &auth,
                a.buyer_token.as_ref(),
                e.buyer,
                e.mint,
                buyer_gets,
                signer,
            )?;
            pay_token(
                program,
                vault,
                mint,
                &auth,
                a.treasury_token.as_ref(),
                a.config.treasury,
                e.mint,
                fee,
                signer,
            )?;
            pay_token(
                program,
                vault,
                mint,
                &auth,
                a.seller_token.as_ref(),
                e.seller,
                e.mint,
                seller_gets,
                signer,
            )?;
            token::close_account(CpiContext::new_with_signer(
                program.key(),
                CloseAccount {
                    account: vault.to_account_info(),
                    destination: a.seller.to_account_info(),
                    authority: e.to_account_info(),
                },
                signer,
            ))?;
        }
    }
    emit!(EscrowSettled {
        escrow: e.key(),
        order_id: e.order_id,
        outcome,
        by: a.signer.key(),
        to_buyer: buyer_gets,
        to_seller: seller_gets,
        fee,
        at: Clock::get()?.unix_timestamp
    });
    Ok(())
}

/// A token payout from the vault (signed by the escrow PDA), to a token account of `owner` for the escrow's mint.
#[allow(clippy::too_many_arguments)]
fn pay_token<'info>(
    program: &Program<'info, Token>,
    vault: &Account<'info, TokenAccount>,
    mint: &Account<'info, Mint>,
    authority: &AccountInfo<'info>,
    to: Option<&Account<'info, TokenAccount>>,
    owner: Pubkey,
    want_mint: Pubkey,
    amount: u64,
    signer: &[&[&[u8]]],
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    let to = to.ok_or(EscrowError::TokenAccountsMissing)?;
    require_keys_eq!(to.owner, owner, EscrowError::WrongAccount);
    require_keys_eq!(to.mint, want_mint, EscrowError::WrongMint);
    token::transfer_checked(
        CpiContext::new_with_signer(
            program.key(),
            TransferChecked {
                from: vault.to_account_info(),
                mint: mint.to_account_info(),
                to: to.to_account_info(),
                authority: authority.clone(),
            },
            signer,
        ),
        amount,
        mint.decimals,
    )
}

fn move_lamports(from: &AccountInfo, to: &AccountInfo, amount: u64) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    let left = from
        .lamports()
        .checked_sub(amount)
        .ok_or(EscrowError::MathOverflow)?;
    let got = to
        .lamports()
        .checked_add(amount)
        .ok_or(EscrowError::MathOverflow)?;
    **from.try_borrow_mut_lamports()? = left;
    **to.try_borrow_mut_lamports()? = got;
    Ok(())
}

// ---------------------------------------------------------------------------
// Accounts

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, payer = payer, space = 8 + Config::INIT_SPACE, seeds = [CONFIG_SEED], bump)]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub payer: Signer<'info>,
    pub mint: Account<'info, Mint>,
    #[account(constraint = program.programdata_address()? == Some(program_data.key()) @ EscrowError::NotOwner)]
    pub program: Program<'info, crate::program::BerylEscrow>,
    #[account(constraint = program_data.upgrade_authority_address == Some(payer.key()) @ EscrowError::NotOwner)]
    pub program_data: Account<'info, ProgramData>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct OwnerOnly<'info> {
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = owner @ EscrowError::NotOwner)]
    pub config: Account<'info, Config>,
    pub owner: Signer<'info>,
}

#[derive(Accounts)]
pub struct Accept<'info> {
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    pub new_key: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(order_id: [u8; 32])]
pub struct FundSol<'info> {
    #[account(mut)]
    pub seller: Signer<'info>,
    #[account(init, payer = seller, space = 8 + Escrow::INIT_SPACE, seeds = [ESCROW_SEED, seller.key().as_ref(), order_id.as_ref()], bump)]
    pub escrow: Account<'info, Escrow>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(order_id: [u8; 32])]
pub struct FundToken<'info> {
    #[account(mut)]
    pub seller: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(address = config.mint @ EscrowError::WrongMint)]
    pub mint: Account<'info, Mint>,
    #[account(mut, token::mint = mint, token::authority = seller)]
    pub seller_token: Account<'info, TokenAccount>,
    #[account(init, payer = seller, space = 8 + Escrow::INIT_SPACE, seeds = [ESCROW_SEED, seller.key().as_ref(), order_id.as_ref()], bump)]
    pub escrow: Account<'info, Escrow>,
    #[account(init, payer = seller, token::mint = mint, token::authority = escrow, seeds = [VAULT_SEED, escrow.key().as_ref()], bump)]
    pub vault: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Party<'info> {
    pub signer: Signer<'info>,
    #[account(mut, seeds = [ESCROW_SEED, escrow.seller.as_ref(), escrow.order_id.as_ref()], bump = escrow.bump)]
    pub escrow: Account<'info, Escrow>,
}

/// Every way an escrow ends. The token accounts are needed only for a token escrow;
/// the treasury only when a release pays a fee.
#[derive(Accounts)]
pub struct Settle<'info> {
    pub signer: Signer<'info>,
    #[account(mut, seeds = [ESCROW_SEED, escrow.seller.as_ref(), escrow.order_id.as_ref()], bump = escrow.bump, close = seller)]
    pub escrow: Account<'info, Escrow>,
    #[account(mut, address = escrow.seller @ EscrowError::WrongAccount)]
    pub seller: SystemAccount<'info>,
    #[account(mut, address = escrow.buyer @ EscrowError::WrongAccount)]
    pub buyer: SystemAccount<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub treasury: Option<SystemAccount<'info>>,
    #[account(mut)]
    pub vault: Option<Account<'info, TokenAccount>>,
    #[account(mut)]
    pub buyer_token: Option<Account<'info, TokenAccount>>,
    #[account(mut)]
    pub seller_token: Option<Account<'info, TokenAccount>>,
    #[account(mut)]
    pub treasury_token: Option<Account<'info, TokenAccount>>,
    pub mint: Option<Account<'info, Mint>>,
    pub token_program: Option<Program<'info, Token>>,
}

// ---------------------------------------------------------------------------
// Events

#[event]
pub struct ConfigInitialized {
    pub owner: Pubkey,
    pub arbiter: Pubkey,
    pub treasury: Pubkey,
    pub mint: Pubkey,
}

#[event]
pub struct FeeSet {
    pub old_bps: u16,
    pub new_bps: u16,
}

#[event]
pub struct TreasurySet {
    pub old: Pubkey,
    pub new: Pubkey,
}

#[event]
pub struct OwnerProposed {
    pub pending: Option<Pubkey>,
}

#[event]
pub struct OwnerAccepted {
    pub old: Pubkey,
    pub new: Pubkey,
}

#[event]
pub struct ArbiterProposed {
    pub pending: Option<Pubkey>,
}

#[event]
pub struct ArbiterAccepted {
    pub old: Pubkey,
    pub new: Pubkey,
}

#[event]
pub struct EscrowFunded {
    pub escrow: Pubkey,
    pub order_id: [u8; 32],
    pub seller: Pubkey,
    pub buyer: Pubkey,
    /// None = SOL.
    pub mint: Option<Pubkey>,
    pub amount: u64,
    pub fee_bps: u16,
    pub deadline: i64,
    pub at: i64,
}

#[event]
pub struct MarkedPaid {
    pub escrow: Pubkey,
    pub order_id: [u8; 32],
    pub at: i64,
}

#[event]
pub struct DisputeOpened {
    pub escrow: Pubkey,
    pub order_id: [u8; 32],
    pub by: Pubkey,
    pub at: i64,
}

#[event]
pub struct EscrowSettled {
    pub escrow: Pubkey,
    pub order_id: [u8; 32],
    pub outcome: Outcome,
    pub by: Pubkey,
    pub to_buyer: u64,
    pub to_seller: u64,
    pub fee: u64,
    pub at: i64,
}
