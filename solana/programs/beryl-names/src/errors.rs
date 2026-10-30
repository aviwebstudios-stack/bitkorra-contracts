use anchor_lang::prelude::*;

#[error_code]
pub enum NamesError {
    #[msg("A name has 3 to 32 characters.")]
    NameTooShort,
    #[msg("A name has 3 to 32 characters.")]
    NameTooLong,
    #[msg("A name can only use lowercase a-z, 0-9 and the hyphen.")]
    NameBadCharacter,
    #[msg("A name can't start or end with a hyphen.")]
    NameEdgeHyphen,
    #[msg("That name is reserved.")]
    ReservedName,
    #[msg("Only the name's owner can do that.")]
    NotOwner,
    #[msg("Only the program authority can do that.")]
    NotAuthority,
    #[msg("That's already the owner.")]
    SameOwner,
    #[msg("That isn't a valid new owner.")]
    InvalidNewOwner,
    #[msg("That Bitcoin address isn't valid.")]
    InvalidBtcRecord,
    #[msg("The avatar must be an https:// or ipfs:// link of up to 200 characters.")]
    InvalidAvatarUri,
    #[msg("That primary name belongs to another wallet or name.")]
    PrimaryMismatch,
}

impl From<crate::name_rules::NameProblem> for NamesError {
    fn from(p: crate::name_rules::NameProblem) -> Self {
        use crate::name_rules::NameProblem::*;
        match p {
            TooShort => NamesError::NameTooShort,
            TooLong => NamesError::NameTooLong,
            BadCharacter => NamesError::NameBadCharacter,
            EdgeHyphen => NamesError::NameEdgeHyphen,
        }
    }
}
