//! Fuzz-style property tests on the name rules: random strings of every
//! kind against a separate, deliberately plain definition of a valid name.

use beryl_names::name_rules::{
    canonical_form, check_name, is_reserved, MAX_NAME_LEN, MIN_NAME_LEN, RESERVED_NAMES,
};
use proptest::prelude::*;

/// The rules written a second way, to compare against.
fn model_valid(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    let len_ok = s.len() >= MIN_NAME_LEN && s.len() <= MAX_NAME_LEN;
    let chars_ok = chars
        .iter()
        .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '-'));
    let edges_ok = chars.first() != Some(&'-') && chars.last() != Some(&'-');
    len_ok && chars_ok && edges_ok
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4000))]

    #[test]
    fn any_string_is_accepted_exactly_when_the_model_says_so(s in "\\PC{0,40}") {
        prop_assert_eq!(check_name(&s).is_ok(), model_valid(&s));
    }

    #[test]
    fn near_misses_from_the_name_alphabet(s in "[a-z0-9-]{0,36}") {
        prop_assert_eq!(check_name(&s).is_ok(), model_valid(&s));
    }

    #[test]
    fn mixed_case_and_neighbours_of_the_alphabet(s in "[A-Za-z0-9_.\\- ]{1,34}") {
        prop_assert_eq!(check_name(&s).is_ok(), model_valid(&s));
    }

    #[test]
    fn an_accepted_name_is_always_a_valid_pda_seed(s in "[a-z0-9-]{3,32}") {
        if check_name(&s).is_ok() {
            prop_assert!(s.len() <= 32);
        }
    }

    #[test]
    fn canonical_form_is_stable_and_never_produces_uppercase(s in "\\PC{0,40}") {
        let once = canonical_form(&s);
        prop_assert_eq!(canonical_form(&once), once.clone());
        prop_assert!(!once.chars().any(|c| c.is_ascii_uppercase()));
    }

    #[test]
    fn what_a_person_types_differs_from_its_canonical_name_only_by_case_spaces_and_the_ending(s in "[a-z0-9]{3,20}", pad in " {0,3}") {
        let typed = format!("{pad}{}.BERYL{pad}", s.to_uppercase());
        prop_assert_eq!(canonical_form(&typed), s.clone());
        prop_assert!(check_name(&canonical_form(&typed)).is_ok());
    }
}

#[test]
fn reserved_names_cover_the_staff_words() {
    for w in [
        "admin",
        "beryl",
        "beryva",
        "support",
        "security",
        "official",
        "moderator",
    ] {
        assert!(is_reserved(w), "{w}");
    }
    assert!(!is_reserved("vanessa"));
    // No duplicates in the list.
    let mut sorted: Vec<&str> = RESERVED_NAMES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), RESERVED_NAMES.len());
}
