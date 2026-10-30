use anchor_lang::prelude::*;

#[error_code]
pub enum EscrowError {
    #[msg("Only the program owner can do that.")]
    NotOwner,
    #[msg("Only the arbiter can resolve a dispute.")]
    NotArbiter,
    #[msg("Only the seller can do that.")]
    NotSeller,
    #[msg("Only the buyer can do that.")]
    NotBuyer,
    #[msg("Only the buyer or the seller can do that.")]
    NotParty,
    #[msg("The fee can't be more than 2%.")]
    FeeTooHigh,
    #[msg("The amount must be more than zero.")]
    ZeroAmount,
    #[msg("The buyer must be someone other than the seller.")]
    BadBuyer,
    #[msg("The payment window must end in the future, within a day.")]
    BadDeadline,
    #[msg("The escrow isn't in a state that allows this.")]
    WrongState,
    #[msg("The payment window has ended.")]
    DeadlinePassed,
    #[msg("The payment window hasn't ended yet.")]
    DeadlineNotPassed,
    #[msg("That isn't this escrow's buyer, seller or treasury.")]
    WrongAccount,
    #[msg("That token isn't accepted.")]
    WrongMint,
    #[msg("A token escrow needs its token accounts.")]
    TokenAccountsMissing,
    #[msg("The amount is too large.")]
    MathOverflow,
    #[msg("There's no key waiting to accept.")]
    NothingPending,
    #[msg("Only the proposed key can accept.")]
    NotPending,
    #[msg("That isn't a valid key.")]
    InvalidKey,
}
