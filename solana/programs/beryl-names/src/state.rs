use anchor_lang::prelude::*;

pub const CONFIG_SEED: &[u8] = b"config";
pub const NAME_SEED: &[u8] = b"name";
pub const PRIMARY_SEED: &[u8] = b"primary";

pub const MAX_BTC_LEN: usize = 90;
pub const MAX_AVATAR_LEN: usize = 200;

/// One per program: who may register reserved names.
/// PDA ["config"].
#[account]
#[derive(InitSpace)]
pub struct Config {
    pub authority: Pubkey,
    pub bump: u8,
}

/// A registered name. PDA ["name", name bytes]: one account per name, so a
/// name can only exist once, and anyone can find it from the name alone.
/// `owner` comes first so it sits at a fixed offset (8, after the
/// discriminator): "every name this wallet owns" is one getProgramAccounts
/// call with a memcmp filter.
#[account]
#[derive(InitSpace)]
pub struct NameRecord {
    pub owner: Pubkey,
    #[max_len(32)]
    pub name: String,
    pub records: Records,
    pub registered_at: i64,
    pub updated_at: i64,
    pub bump: u8,
}

/// Where the owner wants to be paid, per network, and their picture.
/// Cleared when the name changes owner.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Default, PartialEq, Eq, Debug, InitSpace)]
pub struct Records {
    /// An Ethereum (Sepolia) address, as its 20 bytes.
    pub evm: Option<[u8; 20]>,
    /// A Solana address.
    pub sol: Option<Pubkey>,
    /// A Bitcoin (signet) address, as text.
    #[max_len(90)]
    pub btc: Option<String>,
    /// https:// or ipfs:// only.
    #[max_len(200)]
    pub avatar: Option<String>,
}

/// A wallet's chosen name (reverse lookup). PDA ["primary", wallet].
/// Only trusted when the name's record is still owned by the same wallet:
/// after a transfer the old owner's primary points at a name they no
/// longer own, and resolvers must ignore it.
#[account]
#[derive(InitSpace)]
pub struct PrimaryName {
    pub wallet: Pubkey,
    #[max_len(32)]
    pub name: String,
    pub bump: u8,
}
