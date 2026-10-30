//! Beryl Tips: creator tips and paid posts on Solana devnet (Phase W-2).
//!
//! - **Tips** in SOL or the one accepted SPL token (devnet: Circle's test
//!   USDC). The money goes from the tipper straight to the creator in the
//!   same instruction; the program never holds it. Each tip records a
//!   32-byte reference (a hash of the Beryl post or profile, never
//!   personal data) in its event.
//! - **Paid posts**: a creator registers a post with a price (PDA ["post",
//!   creator, post_id]). A buyer pays the creator directly and gets an
//!   access account (PDA ["access", post, buyer]) that proves the purchase.
//!   The creator can change the price or close and reopen sales; access
//!   accounts are never touched, so buyers keep access.
//! - **Protocol fee**: basis points of every payment to the config's
//!   treasury. 0 by default, at most 5% (fees::MAX_FEE_BPS, enforced here),
//!   settable only by the authority. The authority changes hands in two
//!   steps (propose, then accept), like OpenZeppelin's Ownable2Step on Sepolia.
//!
//! Security: signer and owner checks on every account, every PDA checked
//! against its seeds and stored bump, token accounts checked for the
//! accepted mint and their owner (classic SPL Token only, so no transfer
//! fees or hooks can change what arrives), checked maths, a maximum price
//! from the buyer so a price raised in between can't charge more, no
//! self-tips or self-purchases, and an event for every change.

use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::token::{self, Mint, Token, TokenAccount, TransferChecked};

pub mod errors;
pub mod fees;
pub mod state;

use errors::TipsError;
use state::*;

declare_id!("B9oecLNJC7SCTMbZc6G7UJSwj9mUpL9AwF6r3CFE5CEf");

#[program]
pub mod beryl_tips {
    use super::*;

    /// Creates the config. Only the program's upgrade authority can, and only once.
    pub fn initialize(ctx: Context<Initialize>, authority: Pubkey, treasury: Pubkey) -> Result<()> {
        require_keys_neq!(authority, Pubkey::default(), TipsError::InvalidKey);
        require_keys_neq!(treasury, Pubkey::default(), TipsError::InvalidKey);
        let config = &mut ctx.accounts.config;
        config.authority = authority;
        config.pending_authority = None;
        config.treasury = treasury;
        config.mint = ctx.accounts.mint.key();
        config.fee_bps = 0;
        config.bump = ctx.bumps.config;
        emit!(ConfigInitialized {
            authority,
            treasury,
            mint: config.mint
        });
        Ok(())
    }

    pub fn set_fee(ctx: Context<AuthorityOnly>, fee_bps: u16) -> Result<()> {
        require!(fee_bps <= fees::MAX_FEE_BPS, TipsError::FeeTooHigh);
        let config = &mut ctx.accounts.config;
        let old_bps = config.fee_bps;
        config.fee_bps = fee_bps;
        emit!(FeeSet {
            old_bps,
            new_bps: fee_bps
        });
        Ok(())
    }

    pub fn set_treasury(ctx: Context<AuthorityOnly>, treasury: Pubkey) -> Result<()> {
        require_keys_neq!(treasury, Pubkey::default(), TipsError::InvalidKey);
        let config = &mut ctx.accounts.config;
        let old = config.treasury;
        config.treasury = treasury;
        emit!(TreasurySet { old, new: treasury });
        Ok(())
    }

    /// Step 1 of handing over the authority. Pass the default key to cancel.
    pub fn propose_authority(ctx: Context<AuthorityOnly>, new_authority: Pubkey) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.pending_authority = (new_authority != Pubkey::default()).then_some(new_authority);
        emit!(AuthorityProposed {
            authority: config.authority,
            pending: config.pending_authority
        });
        Ok(())
    }

    /// Step 2: the proposed key signs to take over.
    pub fn accept_authority(ctx: Context<AcceptAuthority>) -> Result<()> {
        let config = &mut ctx.accounts.config;
        let pending = config
            .pending_authority
            .ok_or(TipsError::NoPendingAuthority)?;
        require_keys_eq!(
            pending,
            ctx.accounts.new_authority.key(),
            TipsError::NotPendingAuthority
        );
        let old = config.authority;
        config.authority = pending;
        config.pending_authority = None;
        emit!(AuthorityAccepted { old, new: pending });
        Ok(())
    }

    // -- tips ---------------------------------------------------------------

    pub fn tip_sol(ctx: Context<TipSol>, amount: u64, reference: [u8; 32]) -> Result<()> {
        let a = &ctx.accounts;
        require!(amount > 0, TipsError::ZeroAmount);
        require_keys_neq!(a.tipper.key(), a.creator.key(), TipsError::SelfTip);
        let (to_creator, fee) = split(amount, a.config.fee_bps)?;
        pay_sol(
            &a.system_program,
            &a.tipper,
            a.creator.to_account_info(),
            to_creator,
        )?;
        pay_sol(
            &a.system_program,
            &a.tipper,
            a.treasury.to_account_info(),
            fee,
        )?;
        emit!(TipSent {
            tipper: a.tipper.key(),
            creator: a.creator.key(),
            mint: None,
            amount,
            fee,
            reference,
            at: Clock::get()?.unix_timestamp
        });
        Ok(())
    }

    pub fn tip_token(ctx: Context<TipToken>, amount: u64, reference: [u8; 32]) -> Result<()> {
        let a = &ctx.accounts;
        require!(amount > 0, TipsError::ZeroAmount);
        require_keys_neq!(a.tipper.key(), a.creator.key(), TipsError::SelfTip);
        let (to_creator, fee) = split(amount, a.config.fee_bps)?;
        let treasury_token = treasury_token_for(fee, &a.config, a.treasury_token.as_ref())?;
        pay_token(
            &a.token_program,
            &a.tipper,
            &a.tipper_token,
            &a.mint,
            a.creator_token.to_account_info(),
            to_creator,
        )?;
        if let Some(t) = treasury_token {
            pay_token(
                &a.token_program,
                &a.tipper,
                &a.tipper_token,
                &a.mint,
                t,
                fee,
            )?;
        }
        emit!(TipSent {
            tipper: a.tipper.key(),
            creator: a.creator.key(),
            mint: Some(a.mint.key()),
            amount,
            fee,
            reference,
            at: Clock::get()?.unix_timestamp
        });
        Ok(())
    }

    // -- paid posts ---------------------------------------------------------

    pub fn register_post(
        ctx: Context<RegisterPost>,
        post_id: [u8; 32],
        price: u64,
        currency: Currency,
    ) -> Result<()> {
        require!(price > 0, TipsError::ZeroPrice);
        let now = Clock::get()?.unix_timestamp;
        let post = &mut ctx.accounts.post;
        post.creator = ctx.accounts.creator.key();
        post.post_id = post_id;
        post.currency = currency;
        post.mint = match currency {
            Currency::Sol => Pubkey::default(),
            Currency::Token => ctx.accounts.config.mint,
        };
        post.price = price;
        post.open = true;
        post.sales = 0;
        post.created_at = now;
        post.updated_at = now;
        post.bump = ctx.bumps.post;
        emit!(PostRegistered {
            post: post.key(),
            creator: post.creator,
            post_id,
            currency,
            mint: post.mint,
            price,
            at: now
        });
        Ok(())
    }

    pub fn update_price(ctx: Context<CreatorOnly>, new_price: u64) -> Result<()> {
        require!(new_price > 0, TipsError::ZeroPrice);
        let now = Clock::get()?.unix_timestamp;
        let post = &mut ctx.accounts.post;
        let old_price = post.price;
        post.price = new_price;
        post.updated_at = now;
        emit!(PriceChanged {
            post: post.key(),
            old_price,
            new_price,
            at: now
        });
        Ok(())
    }

    /// Closes (false) or reopens (true) sales. Existing access stays.
    pub fn set_sales_open(ctx: Context<CreatorOnly>, open: bool) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;
        let post = &mut ctx.accounts.post;
        post.open = open;
        post.updated_at = now;
        emit!(SalesStatusChanged {
            post: post.key(),
            open,
            at: now
        });
        Ok(())
    }

    /// Buys a SOL-priced post. `max_price` is the price the buyer saw: a
    /// higher price now fails instead of charging more.
    pub fn buy_sol(ctx: Context<BuySol>, max_price: u64) -> Result<()> {
        let price = check_sale(
            &ctx.accounts.post,
            &ctx.accounts.buyer,
            Currency::Sol,
            max_price,
        )?;
        let (to_creator, fee) = split(price, ctx.accounts.config.fee_bps)?;
        let (post_key, access) = (ctx.accounts.post.key(), ctx.accounts.access.key());
        record_sale(
            &mut ctx.accounts.post,
            post_key,
            &mut ctx.accounts.access,
            ctx.accounts.buyer.key(),
            ctx.bumps.access,
        )?;
        let a = &ctx.accounts;
        pay_sol(
            &a.system_program,
            &a.buyer,
            a.creator.to_account_info(),
            to_creator,
        )?;
        pay_sol(
            &a.system_program,
            &a.buyer,
            a.treasury.to_account_info(),
            fee,
        )?;
        emit!(PostPurchased {
            post: a.post.key(),
            creator: a.post.creator,
            buyer: a.buyer.key(),
            access,
            mint: None,
            price,
            fee,
            at: a.access.purchased_at
        });
        Ok(())
    }

    pub fn buy_token(ctx: Context<BuyToken>, max_price: u64) -> Result<()> {
        let price = check_sale(
            &ctx.accounts.post,
            &ctx.accounts.buyer,
            Currency::Token,
            max_price,
        )?;
        require_keys_eq!(
            ctx.accounts.post.mint,
            ctx.accounts.mint.key(),
            TipsError::WrongMint
        );
        let (to_creator, fee) = split(price, ctx.accounts.config.fee_bps)?;
        let (post_key, access) = (ctx.accounts.post.key(), ctx.accounts.access.key());
        record_sale(
            &mut ctx.accounts.post,
            post_key,
            &mut ctx.accounts.access,
            ctx.accounts.buyer.key(),
            ctx.bumps.access,
        )?;
        let a = &ctx.accounts;
        let treasury_token = treasury_token_for(fee, &a.config, a.treasury_token.as_ref())?;
        pay_token(
            &a.token_program,
            &a.buyer,
            &a.buyer_token,
            &a.mint,
            a.creator_token.to_account_info(),
            to_creator,
        )?;
        if let Some(t) = treasury_token {
            pay_token(&a.token_program, &a.buyer, &a.buyer_token, &a.mint, t, fee)?;
        }
        emit!(PostPurchased {
            post: a.post.key(),
            creator: a.post.creator,
            buyer: a.buyer.key(),
            access,
            mint: Some(a.mint.key()),
            price,
            fee,
            at: a.access.purchased_at
        });
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers

fn split(amount: u64, fee_bps: u16) -> Result<(u64, u64)> {
    fees::split(amount, fee_bps).ok_or_else(|| error!(TipsError::MathOverflow))
}

fn pay_sol<'info>(
    system: &Program<'info, System>,
    from: &Signer<'info>,
    to: AccountInfo<'info>,
    lamports: u64,
) -> Result<()> {
    if lamports == 0 {
        return Ok(());
    }
    system_program::transfer(
        CpiContext::new(
            system.key(),
            system_program::Transfer {
                from: from.to_account_info(),
                to,
            },
        ),
        lamports,
    )
}

fn pay_token<'info>(
    token_program: &Program<'info, Token>,
    authority: &Signer<'info>,
    from: &Account<'info, TokenAccount>,
    mint: &Account<'info, Mint>,
    to: AccountInfo<'info>,
    amount: u64,
) -> Result<()> {
    if amount == 0 {
        return Ok(());
    }
    token::transfer_checked(
        CpiContext::new(
            token_program.key(),
            TransferChecked {
                from: from.to_account_info(),
                mint: mint.to_account_info(),
                to,
                authority: authority.to_account_info(),
            },
        ),
        amount,
        mint.decimals,
    )
}

/// The treasury's token account, required (and checked) only when there's a fee to pay into it.
fn treasury_token_for<'info>(
    fee: u64,
    config: &Config,
    treasury_token: Option<&Account<'info, TokenAccount>>,
) -> Result<Option<AccountInfo<'info>>> {
    if fee == 0 {
        return Ok(None);
    }
    let t = treasury_token.ok_or(TipsError::TreasuryAccountMissing)?;
    require_keys_eq!(t.owner, config.treasury, TipsError::WrongTreasury);
    require_keys_eq!(t.mint, config.mint, TipsError::WrongMint);
    Ok(Some(t.to_account_info()))
}

fn check_sale(post: &Post, buyer: &Signer, currency: Currency, max_price: u64) -> Result<u64> {
    require!(post.open, TipsError::SalesClosed);
    require!(post.currency == currency, TipsError::WrongCurrency);
    require_keys_neq!(buyer.key(), post.creator, TipsError::SelfPurchase);
    require!(post.price <= max_price, TipsError::PriceAboveMax);
    Ok(post.price)
}

/// Effects before the transfers: the sale is counted and the access account filled in.
fn record_sale(
    post: &mut Post,
    post_key: Pubkey,
    access: &mut Access,
    buyer: Pubkey,
    bump: u8,
) -> Result<()> {
    post.sales = post.sales.checked_add(1).ok_or(TipsError::MathOverflow)?;
    access.post = post_key;
    access.buyer = buyer;
    access.price = post.price;
    access.mint = post.mint;
    access.purchased_at = Clock::get()?.unix_timestamp;
    access.bump = bump;
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
    /// The one SPL token accepted. Must be a classic SPL Token mint.
    pub mint: Account<'info, Mint>,
    /// This program: its ProgramData must be the one below.
    #[account(constraint = program.programdata_address()? == Some(program_data.key()) @ TipsError::NotAuthority)]
    pub program: Program<'info, crate::program::BerylTips>,
    /// Only the program's upgrade authority can set up the config.
    #[account(constraint = program_data.upgrade_authority_address == Some(payer.key()) @ TipsError::NotAuthority)]
    pub program_data: Account<'info, ProgramData>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct AuthorityOnly<'info> {
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = authority @ TipsError::NotAuthority)]
    pub config: Account<'info, Config>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct AcceptAuthority<'info> {
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    pub new_authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct TipSol<'info> {
    #[account(mut)]
    pub tipper: Signer<'info>,
    /// A wallet (owned by the system program), never a program account.
    #[account(mut)]
    pub creator: SystemAccount<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut, address = config.treasury @ TipsError::WrongTreasury)]
    pub treasury: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct TipToken<'info> {
    pub tipper: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(address = config.mint @ TipsError::WrongMint)]
    pub mint: Account<'info, Mint>,
    #[account(mut, token::mint = mint, token::authority = tipper)]
    pub tipper_token: Account<'info, TokenAccount>,
    pub creator: SystemAccount<'info>,
    #[account(mut, token::mint = mint, token::authority = creator)]
    pub creator_token: Account<'info, TokenAccount>,
    /// Needed only while the fee is above 0; checked in the instruction.
    #[account(mut)]
    pub treasury_token: Option<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
#[instruction(post_id: [u8; 32])]
pub struct RegisterPost<'info> {
    #[account(
        init,
        payer = creator,
        space = 8 + Post::INIT_SPACE,
        seeds = [POST_SEED, creator.key().as_ref(), post_id.as_ref()],
        bump
    )]
    pub post: Account<'info, Post>,
    #[account(mut)]
    pub creator: Signer<'info>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CreatorOnly<'info> {
    #[account(
        mut,
        seeds = [POST_SEED, creator.key().as_ref(), post.post_id.as_ref()],
        bump = post.bump,
        has_one = creator @ TipsError::NotAuthority
    )]
    pub post: Account<'info, Post>,
    pub creator: Signer<'info>,
}

#[derive(Accounts)]
pub struct BuySol<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,
    #[account(
        mut,
        seeds = [POST_SEED, post.creator.as_ref(), post.post_id.as_ref()],
        bump = post.bump
    )]
    pub post: Account<'info, Post>,
    #[account(mut, address = post.creator @ TipsError::WrongCreator)]
    pub creator: SystemAccount<'info>,
    #[account(
        init,
        payer = buyer,
        space = 8 + Access::INIT_SPACE,
        seeds = [ACCESS_SEED, post.key().as_ref(), buyer.key().as_ref()],
        bump
    )]
    pub access: Account<'info, Access>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut, address = config.treasury @ TipsError::WrongTreasury)]
    pub treasury: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct BuyToken<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,
    #[account(
        mut,
        seeds = [POST_SEED, post.creator.as_ref(), post.post_id.as_ref()],
        bump = post.bump
    )]
    pub post: Account<'info, Post>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(address = config.mint @ TipsError::WrongMint)]
    pub mint: Account<'info, Mint>,
    #[account(mut, token::mint = mint, token::authority = buyer)]
    pub buyer_token: Account<'info, TokenAccount>,
    #[account(address = post.creator @ TipsError::WrongCreator)]
    pub creator: SystemAccount<'info>,
    #[account(mut, token::mint = mint, token::authority = creator)]
    pub creator_token: Account<'info, TokenAccount>,
    #[account(
        init,
        payer = buyer,
        space = 8 + Access::INIT_SPACE,
        seeds = [ACCESS_SEED, post.key().as_ref(), buyer.key().as_ref()],
        bump
    )]
    pub access: Account<'info, Access>,
    /// Needed only while the fee is above 0; checked in the instruction.
    #[account(mut)]
    pub treasury_token: Option<Account<'info, TokenAccount>>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

// ---------------------------------------------------------------------------
// Events

#[event]
pub struct ConfigInitialized {
    pub authority: Pubkey,
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
pub struct AuthorityProposed {
    pub authority: Pubkey,
    pub pending: Option<Pubkey>,
}

#[event]
pub struct AuthorityAccepted {
    pub old: Pubkey,
    pub new: Pubkey,
}

#[event]
pub struct TipSent {
    pub tipper: Pubkey,
    pub creator: Pubkey,
    /// None = SOL.
    pub mint: Option<Pubkey>,
    /// What the tipper paid in total; the creator got `amount - fee`.
    pub amount: u64,
    pub fee: u64,
    pub reference: [u8; 32],
    pub at: i64,
}

#[event]
pub struct PostRegistered {
    pub post: Pubkey,
    pub creator: Pubkey,
    pub post_id: [u8; 32],
    pub currency: Currency,
    pub mint: Pubkey,
    pub price: u64,
    pub at: i64,
}

#[event]
pub struct PriceChanged {
    pub post: Pubkey,
    pub old_price: u64,
    pub new_price: u64,
    pub at: i64,
}

#[event]
pub struct SalesStatusChanged {
    pub post: Pubkey,
    pub open: bool,
    pub at: i64,
}

#[event]
pub struct PostPurchased {
    pub post: Pubkey,
    pub creator: Pubkey,
    pub buyer: Pubkey,
    pub access: Pubkey,
    /// None = SOL.
    pub mint: Option<Pubkey>,
    pub price: u64,
    pub fee: u64,
    pub at: i64,
}
