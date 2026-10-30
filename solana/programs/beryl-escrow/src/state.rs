use anchor_lang::prelude::*;

pub const CONFIG_SEED: &[u8] = b"config";
pub const ESCROW_SEED: &[u8] = b"escrow";
pub const VAULT_SEED: &[u8] = b"vault";

/// The fee's hard cap: 2%. Paid only on a release to the buyer.
pub const MAX_FEE_BPS: u16 = 200;
pub const BPS: u64 = 10_000;
/// The longest payment window an escrow can have.
pub const MAX_WINDOW_SECONDS: i64 = 24 * 60 * 60;

/// One per program. PDA ["config"].
#[account]
#[derive(InitSpace)]
pub struct Config {
    /// Sets the fee, the treasury and the arbiter; hands itself over in two steps.
    pub owner: Pubkey,
    pub pending_owner: Option<Pubkey>,
    /// The only key that can resolve a disputed escrow. Changed in two steps.
    pub arbiter: Pubkey,
    pub pending_arbiter: Option<Pubkey>,
    pub treasury: Pubkey,
    /// The one SPL token accepted (devnet: Circle's USDC). Classic SPL Token only.
    pub mint: Pubkey,
    /// Basis points taken from a release to the buyer, never above MAX_FEE_BPS.
    pub fee_bps: u16,
    pub bump: u8,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug, InitSpace)]
pub enum Currency {
    Sol,
    Token,
}

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug, InitSpace)]
pub enum EscrowState {
    /// The seller's crypto is locked; the buyer hasn't said they paid.
    Funded,
    /// The buyer said they paid before the deadline: the seller can no longer reclaim.
    Paid,
    /// Either side opened a dispute after "paid": only the arbiter (or the seller releasing) ends it.
    Disputed,
}

/// How an escrow ended (in the EscrowSettled event; the account is closed).
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// The seller released to the buyer.
    Released,
    /// The deadline passed without "paid", and the seller took it back.
    Refunded,
    /// The buyer cancelled before paying: back to the seller.
    Cancelled,
    /// The arbiter resolved a dispute for the buyer.
    ResolvedBuyer,
    /// The arbiter resolved a dispute for the seller.
    ResolvedSeller,
}

/// One order's escrow. PDA ["escrow", seller, order_id]. `order_id` is a hash
/// of Beryl's order id, never anything personal. SOL is held in this account's
/// own lamports; tokens in the vault PDA ["vault", escrow].
#[account]
#[derive(InitSpace)]
pub struct Escrow {
    pub seller: Pubkey,
    pub buyer: Pubkey,
    pub order_id: [u8; 32],
    pub currency: Currency,
    /// The token's mint; the default key for SOL.
    pub mint: Pubkey,
    pub amount: u64,
    /// The fee at funding time: a later fee change never touches an open escrow.
    pub fee_bps: u16,
    /// Unix time by which the buyer must mark it paid.
    pub deadline: i64,
    pub state: EscrowState,
    pub funded_at: i64,
    pub bump: u8,
    pub vault_bump: u8,
}

/// (to the buyer, the fee) of a release. Rounded down: the buyer gets any fraction.
pub fn release_split(amount: u64, fee_bps: u16) -> Option<(u64, u64)> {
    if fee_bps > MAX_FEE_BPS {
        return None;
    }
    let fee = u64::try_from((amount as u128).checked_mul(fee_bps as u128)? / BPS as u128).ok()?;
    Some((amount.checked_sub(fee)?, fee))
}
