use anchor_lang::prelude::*;

#[error_code]
pub enum TipsError {
    #[msg("Only the program authority can do that.")]
    NotAuthority,
    #[msg("The fee can't be more than 5%.")]
    FeeTooHigh,
    #[msg("The amount must be more than zero.")]
    ZeroAmount,
    #[msg("You can't tip yourself.")]
    SelfTip,
    #[msg("You can't buy your own post.")]
    SelfPurchase,
    #[msg("That isn't the treasury.")]
    WrongTreasury,
    #[msg("The treasury's token account is needed while there's a fee.")]
    TreasuryAccountMissing,
    #[msg("That token isn't accepted.")]
    WrongMint,
    #[msg("This post is sold in another currency.")]
    WrongCurrency,
    #[msg("Sales of this post are closed.")]
    SalesClosed,
    #[msg("The price went up since you looked.")]
    PriceAboveMax,
    #[msg("The price must be more than zero.")]
    ZeroPrice,
    #[msg("The amount is too large.")]
    MathOverflow,
    #[msg("There's no authority waiting to accept.")]
    NoPendingAuthority,
    #[msg("Only the proposed authority can accept.")]
    NotPendingAuthority,
    #[msg("That isn't a valid authority or treasury.")]
    InvalidKey,
    #[msg("The payment must go to the post's creator.")]
    WrongCreator,
}
