//! The rules for a Beryl name (the part before ".beryl").
//!
//! - 3 to 32 characters (32 is also Solana's limit for one PDA seed);
//! - only lowercase a-z, digits 0-9 and the hyphen;
//! - no hyphen at the start or the end;
//! - not on the reserved list, unless the program authority registers it.
//!
//! The program never normalizes: it only accepts names that are already in
//! their one canonical form. Clients lowercase and trim before sending
//! (lib/beryva/names in the app does the same, with the same tests), so
//! "Vanessa" and "vanessa" can never be two different accounts.
//!
//! Pure functions with no Anchor types, so they can be fuzzed directly.

pub const MIN_NAME_LEN: usize = 3;
pub const MAX_NAME_LEN: usize = 32;

/// Names nobody but the program authority can register: Beryl's own
/// words, and words that could pass for staff or for the system.
pub const RESERVED_NAMES: &[&str] = &[
    "admin",
    "administrator",
    "beryl",
    "beryva",
    "support",
    "help",
    "helpdesk",
    "security",
    "official",
    "staff",
    "team",
    "moderator",
    "mod",
    "root",
    "system",
    "wallet",
    "names",
    "payments",
    "billing",
    "verify",
    "verified",
    "settings",
    "login",
    "signup",
    "api",
    "www",
    "mail",
    "info",
    "contact",
    "abuse",
    "legal",
    "compliance",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameProblem {
    TooShort,
    TooLong,
    BadCharacter,
    EdgeHyphen,
}

/// Checks a name against the shape rules (not the reserved list).
pub fn check_name(name: &str) -> Result<(), NameProblem> {
    let bytes = name.as_bytes();
    // Length in bytes: only ASCII is allowed, so bytes and characters are the same here.
    if bytes.len() < MIN_NAME_LEN {
        return Err(NameProblem::TooShort);
    }
    if bytes.len() > MAX_NAME_LEN {
        return Err(NameProblem::TooLong);
    }
    if !bytes
        .iter()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
    {
        return Err(NameProblem::BadCharacter);
    }
    if bytes.first() == Some(&b'-') || bytes.last() == Some(&b'-') {
        return Err(NameProblem::EdgeHyphen);
    }
    Ok(())
}

pub fn is_reserved(name: &str) -> bool {
    RESERVED_NAMES.contains(&name)
}

/// What a client should send for what a person typed: trimmed, lowercased,
/// without a ".beryl" ending. Not used by the program itself (it rejects
/// anything not already canonical); kept here so the Rust tests and the
/// app's TypeScript follow one definition.
pub fn canonical_form(input: &str) -> String {
    let lowered = input.trim().to_ascii_lowercase();
    lowered
        .strip_suffix(".beryl")
        .unwrap_or(&lowered)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_the_plain_cases() {
        for ok in [
            "abc",
            "vanessa",
            "a-b",
            "x1y",
            "123",
            "a".repeat(32).as_str(),
            "lagos-2026",
        ] {
            assert_eq!(check_name(ok), Ok(()), "{ok}");
        }
    }

    #[test]
    fn refuses_the_rest() {
        assert_eq!(check_name("ab"), Err(NameProblem::TooShort));
        assert_eq!(check_name(&"a".repeat(33)), Err(NameProblem::TooLong));
        assert_eq!(check_name("Vanessa"), Err(NameProblem::BadCharacter));
        assert_eq!(check_name("van_essa"), Err(NameProblem::BadCharacter));
        assert_eq!(check_name("van essa"), Err(NameProblem::BadCharacter));
        assert_eq!(check_name("vanessa.beryl"), Err(NameProblem::BadCharacter));
        assert_eq!(check_name("vané"), Err(NameProblem::BadCharacter));
        assert_eq!(check_name("-abc"), Err(NameProblem::EdgeHyphen));
        assert_eq!(check_name("abc-"), Err(NameProblem::EdgeHyphen));
    }

    #[test]
    fn reserved_names_are_themselves_valid_names() {
        for r in RESERVED_NAMES {
            assert_eq!(check_name(r), Ok(()), "{r}");
        }
    }

    #[test]
    fn canonical_form_lowercases_trims_and_drops_the_ending() {
        assert_eq!(canonical_form("  Vanessa.BERYL "), "vanessa");
        assert_eq!(canonical_form("ada"), "ada");
    }
}
