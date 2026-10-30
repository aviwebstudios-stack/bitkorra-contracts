use anchor_lang::prelude::*;

pub const CONFIG_SEED: &[u8] = b"config";
pub const POST_SEED: &[u8] = b"post";
pub const ACCESS_SEED: &[u8] = b"access";

/// One per program. PDA ["config"].
#[account]
#[derive(InitSpace)]
pub struct Config {
    /// Sets the fee and the treasury, and hands itself over in two steps.
    pub authority: Pubkey,
    /// Proposed by `propose_authority`; becomes the authority when it signs `accept_authority`.
    pub pending_authority: Option<Pubkey>,
    /// Receives the fee (a wallet; its token account for token payments).
    pub treasury: Pubkey,
    /// The one SPL token accepted (devnet: Circle's test USDC). Classic SPL Token only.
    pub mint: Pubkey,
    /// Basis points of each payment, 0 by default, never above fees::MAX_FEE_BPS.
    pub fee_bps: u16,
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug, InitSpace)]
pub enum Currency {
    Sol,
    Token,
}

/// A paid post. PDA ["post", creator, post_id]. `post_id` is a hash of
/// Beryl's post id, never the content or anything personal. `creator` comes
/// first (offset 8) so "this creator's posts" is one memcmp filter.
#[account]
#[derive(InitSpace)]
pub struct Post {
    pub creator: Pubkey,
    pub post_id: [u8; 32],
    pub currency: Currency,
    /// The token's mint for Currency::Token; the default key for SOL.
    pub mint: Pubkey,
    /// In lamports, or the token's smallest unit.
    pub price: u64,
    /// false = sales closed. People who bought keep their access.
    pub open: bool,
    pub sales: u64,
    pub created_at: i64,
    pub updated_at: i64,
    pub bump: u8,
}

/// Proof that `buyer` paid for `post`. PDA ["access", post, buyer]: one per
/// buyer and post, so nobody pays twice, and anyone can check it.
#[account]
#[derive(InitSpace)]
pub struct Access {
    pub post: Pubkey,
    pub buyer: Pubkey,
    pub price: u64,
    pub mint: Pubkey,
    pub purchased_at: i64,
    pub bump: u8,
}
