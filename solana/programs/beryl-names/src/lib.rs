//! Beryl Names: usernames like `vanessa.beryl` as Solana accounts owned by
//! a wallet (Solana devnet; Phase W-1).
//!
//! - One account per name, at the PDA ["name", name]: a name exists at
//!   most once, and anyone can find it from the name.
//! - The owner can transfer it (its records are cleared), set its records
//!   (their Ethereum, Solana and Bitcoin addresses and an avatar link),
//!   set it as their wallet's primary name (reverse lookup at the PDA
//!   ["primary", wallet]) and release it (its rent comes back).
//! - Reserved names (name_rules::RESERVED_NAMES) can only be registered by
//!   the authority in the ["config"] account, which only the program's
//!   upgrade authority can create.
//!
//! Security: every instruction that changes a name checks its owner
//! (has_one), every PDA is checked against its seeds and stored bump,
//! there are no unchecked accounts, and every change emits an event.

use anchor_lang::prelude::*;

pub mod errors;
pub mod name_rules;
pub mod state;

use errors::NamesError;
use state::*;

declare_id!("92jgXoEDydnpZhUFbcqoFf7aPasLGPP1S2E7CcpLYuPR");

#[program]
pub mod beryl_names {
    use super::*;

    /// Creates the config. Only the program's upgrade authority can, and only once.
    pub fn initialize(ctx: Context<Initialize>, authority: Pubkey) -> Result<()> {
        let config = &mut ctx.accounts.config;
        config.authority = authority;
        config.bump = ctx.bumps.config;
        emit!(AuthoritySet { authority });
        Ok(())
    }

    /// Hands the reserved-names authority to another key.
    pub fn set_authority(ctx: Context<SetAuthority>, new_authority: Pubkey) -> Result<()> {
        require_keys_neq!(
            new_authority,
            Pubkey::default(),
            NamesError::InvalidNewOwner
        );
        ctx.accounts.config.authority = new_authority;
        emit!(AuthoritySet {
            authority: new_authority
        });
        Ok(())
    }

    pub fn register(ctx: Context<Register>, name: String) -> Result<()> {
        name_rules::check_name(&name).map_err(NamesError::from)?;
        if name_rules::is_reserved(&name) {
            require_keys_eq!(
                ctx.accounts.owner.key(),
                ctx.accounts.config.authority,
                NamesError::ReservedName
            );
        }
        let now = Clock::get()?.unix_timestamp;
        let record = &mut ctx.accounts.name_record;
        record.name = name;
        record.owner = ctx.accounts.owner.key();
        record.records = Records::default();
        record.registered_at = now;
        record.updated_at = now;
        record.bump = ctx.bumps.name_record;
        emit!(NameRegistered {
            name: record.name.clone(),
            owner: record.owner,
            at: now
        });
        Ok(())
    }

    /// Gives the name to another wallet. Its records are cleared, so money
    /// sent to the name can never reach the old owner's addresses.
    pub fn transfer_name(ctx: Context<OwnerOnly>, new_owner: Pubkey) -> Result<()> {
        let record = &mut ctx.accounts.name_record;
        require_keys_neq!(new_owner, Pubkey::default(), NamesError::InvalidNewOwner);
        require_keys_neq!(new_owner, record.owner, NamesError::SameOwner);
        let from = record.owner;
        let now = Clock::get()?.unix_timestamp;
        record.owner = new_owner;
        record.records = Records::default();
        record.updated_at = now;
        emit!(NameTransferred {
            name: record.name.clone(),
            from,
            to: new_owner,
            at: now
        });
        Ok(())
    }

    /// Replaces the name's records (pass None to clear one).
    pub fn update_records(ctx: Context<OwnerOnly>, records: Records) -> Result<()> {
        if let Some(btc) = &records.btc {
            require!(valid_btc(btc), NamesError::InvalidBtcRecord);
        }
        if let Some(uri) = &records.avatar {
            require!(valid_avatar(uri), NamesError::InvalidAvatarUri);
        }
        let now = Clock::get()?.unix_timestamp;
        let record = &mut ctx.accounts.name_record;
        record.records = records;
        record.updated_at = now;
        emit!(RecordsUpdated {
            name: record.name.clone(),
            owner: record.owner,
            at: now
        });
        Ok(())
    }

    /// Makes this name the wallet's primary name (one per wallet).
    pub fn set_primary(ctx: Context<SetPrimary>) -> Result<()> {
        let primary = &mut ctx.accounts.primary;
        primary.wallet = ctx.accounts.owner.key();
        primary.name = ctx.accounts.name_record.name.clone();
        primary.bump = ctx.bumps.primary;
        emit!(PrimarySet {
            wallet: primary.wallet,
            name: primary.name.clone()
        });
        Ok(())
    }

    /// Removes the wallet's primary name (its rent comes back).
    pub fn clear_primary(ctx: Context<ClearPrimary>) -> Result<()> {
        emit!(PrimaryCleared {
            wallet: ctx.accounts.owner.key(),
            name: ctx.accounts.primary.name.clone()
        });
        Ok(())
    }

    /// Gives the name up: its account closes and its rent goes back to the
    /// owner. If it was their primary name, pass that account and it closes too.
    pub fn release(ctx: Context<Release>) -> Result<()> {
        let name = ctx.accounts.name_record.name.clone();
        if let Some(primary) = &ctx.accounts.primary {
            require!(primary.name == name, NamesError::PrimaryMismatch);
            primary.close(ctx.accounts.owner.to_account_info())?;
        }
        emit!(NameReleased {
            name,
            owner: ctx.accounts.owner.key(),
            at: Clock::get()?.unix_timestamp
        });
        Ok(())
    }
}

/// Bitcoin signet and testnet: bech32 ("tb1…", lowercase) or base58 ("m", "n", "2"), by shape.
/// The app checks the checksum before it sends; here it only keeps junk out.
fn valid_btc(s: &str) -> bool {
    let len = s.len();
    if !(26..=state::MAX_BTC_LEN).contains(&len) {
        return false;
    }
    if let Some(rest) = s.strip_prefix("tb1") {
        return rest
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    }
    matches!(s.as_bytes()[0], b'm' | b'n' | b'2')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() && !matches!(b, b'0' | b'O' | b'I' | b'l'))
}

fn valid_avatar(s: &str) -> bool {
    s.len() <= state::MAX_AVATAR_LEN
        && (s.starts_with("https://") || s.starts_with("ipfs://"))
        && s.len() > 8
        && s.bytes().all(|b| b.is_ascii_graphic())
}

// ---------------------------------------------------------------------------
// Accounts

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, payer = payer, space = 8 + Config::INIT_SPACE, seeds = [CONFIG_SEED], bump)]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub payer: Signer<'info>,
    /// This program: its ProgramData must be the one below.
    #[account(constraint = program.programdata_address()? == Some(program_data.key()) @ NamesError::NotAuthority)]
    pub program: Program<'info, crate::program::BerylNames>,
    /// Only the program's upgrade authority can set up the config.
    #[account(constraint = program_data.upgrade_authority_address == Some(payer.key()) @ NamesError::NotAuthority)]
    pub program_data: Account<'info, ProgramData>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct SetAuthority<'info> {
    #[account(mut, seeds = [CONFIG_SEED], bump = config.bump, has_one = authority @ NamesError::NotAuthority)]
    pub config: Account<'info, Config>,
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
#[instruction(name: String)]
pub struct Register<'info> {
    #[account(
        init,
        payer = owner,
        space = 8 + NameRecord::INIT_SPACE,
        seeds = [NAME_SEED, name.as_bytes()],
        bump
    )]
    pub name_record: Account<'info, NameRecord>,
    #[account(seeds = [CONFIG_SEED], bump = config.bump)]
    pub config: Account<'info, Config>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct OwnerOnly<'info> {
    #[account(
        mut,
        seeds = [NAME_SEED, name_record.name.as_bytes()],
        bump = name_record.bump,
        has_one = owner @ NamesError::NotOwner
    )]
    pub name_record: Account<'info, NameRecord>,
    pub owner: Signer<'info>,
}

#[derive(Accounts)]
pub struct SetPrimary<'info> {
    #[account(
        seeds = [NAME_SEED, name_record.name.as_bytes()],
        bump = name_record.bump,
        has_one = owner @ NamesError::NotOwner
    )]
    pub name_record: Account<'info, NameRecord>,
    #[account(
        init_if_needed,
        payer = owner,
        space = 8 + PrimaryName::INIT_SPACE,
        seeds = [PRIMARY_SEED, owner.key().as_ref()],
        bump
    )]
    pub primary: Account<'info, PrimaryName>,
    #[account(mut)]
    pub owner: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct ClearPrimary<'info> {
    #[account(
        mut,
        close = owner,
        seeds = [PRIMARY_SEED, owner.key().as_ref()],
        bump = primary.bump,
        constraint = primary.wallet == owner.key() @ NamesError::PrimaryMismatch
    )]
    pub primary: Account<'info, PrimaryName>,
    #[account(mut)]
    pub owner: Signer<'info>,
}

#[derive(Accounts)]
pub struct Release<'info> {
    #[account(
        mut,
        close = owner,
        seeds = [NAME_SEED, name_record.name.as_bytes()],
        bump = name_record.bump,
        has_one = owner @ NamesError::NotOwner
    )]
    pub name_record: Account<'info, NameRecord>,
    #[account(
        mut,
        seeds = [PRIMARY_SEED, owner.key().as_ref()],
        bump = primary.bump,
        constraint = primary.wallet == owner.key() @ NamesError::PrimaryMismatch
    )]
    pub primary: Option<Account<'info, PrimaryName>>,
    #[account(mut)]
    pub owner: Signer<'info>,
}

// ---------------------------------------------------------------------------
// Events

#[event]
pub struct AuthoritySet {
    pub authority: Pubkey,
}

#[event]
pub struct NameRegistered {
    pub name: String,
    pub owner: Pubkey,
    pub at: i64,
}

#[event]
pub struct NameTransferred {
    pub name: String,
    pub from: Pubkey,
    pub to: Pubkey,
    pub at: i64,
}

#[event]
pub struct RecordsUpdated {
    pub name: String,
    pub owner: Pubkey,
    pub at: i64,
}

#[event]
pub struct PrimarySet {
    pub wallet: Pubkey,
    pub name: String,
}

#[event]
pub struct PrimaryCleared {
    pub wallet: Pubkey,
    pub name: String,
}

#[event]
pub struct NameReleased {
    pub name: String,
    pub owner: Pubkey,
    pub at: i64,
}
