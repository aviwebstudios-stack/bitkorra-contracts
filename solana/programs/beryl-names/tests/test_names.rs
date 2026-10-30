//! Beryl Names, end to end in LiteSVM: every instruction, and every way it
//! must refuse (taken name, bad characters, reserved name, wrong owner,
//! wrong PDA, wrong authority).

mod common;

use anchor_lang::prelude::Pubkey;
use beryl_names::state::Records;
use common::*;
use solana_signer::Signer;

fn records() -> Records {
    Records {
        evm: Some([0x11; 20]),
        // Fixed, so two calls give equal records.
        sol: Some(Pubkey::new_from_array([0x22; 32])),
        btc: Some("tb1q6u2zjsv8rdgtu9suvrgqmgds5kklllpqfjkn2u".to_string()),
        avatar: Some(
            "ipfs://bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi".to_string(),
        ),
    }
}

// ---------------------------------------------------------------------------
// initialize and set_authority

#[test]
fn only_the_upgrade_authority_can_initialize_and_only_once() {
    let mut env = fresh();
    let stranger = env.user();
    let authority = env.authority.pubkey();
    assert_error(
        env.send(
            &[initialize_ix(&stranger.pubkey(), authority)],
            &[&stranger],
        ),
        "NotAuthority",
    );
    // A ProgramData account that isn't this program's is refused.
    let fake = Pubkey::new_unique();
    let ua = env.upgrade_authority.insecure_clone();
    assert!(env
        .send(&[initialize_ix_with(&ua.pubkey(), authority, fake)], &[&ua])
        .is_err());
    env.send(&[initialize_ix(&ua.pubkey(), authority)], &[&ua])
        .unwrap();
    assert_eq!(env.config().unwrap().authority, authority);
    // Once: the config account already exists.
    assert!(env
        .send(&[initialize_ix(&ua.pubkey(), stranger.pubkey())], &[&ua])
        .is_err());
    assert_eq!(env.config().unwrap().authority, authority);
}

#[test]
fn the_authority_can_hand_over_and_nobody_else_can() {
    let mut env = ready();
    let stranger = env.user();
    assert_error(
        env.send(
            &[set_authority_ix(&stranger.pubkey(), stranger.pubkey())],
            &[&stranger],
        ),
        "NotAuthority",
    );
    let auth = env.authority.insecure_clone();
    assert_error(
        env.send(
            &[set_authority_ix(&auth.pubkey(), Pubkey::default())],
            &[&auth],
        ),
        "InvalidNewOwner",
    );
    env.send(
        &[set_authority_ix(&auth.pubkey(), stranger.pubkey())],
        &[&auth],
    )
    .unwrap();
    assert_eq!(env.config().unwrap().authority, stranger.pubkey());
}

// ---------------------------------------------------------------------------
// register

#[test]
fn registers_a_name_to_the_signer() {
    let mut env = ready();
    let ada = env.user();
    env.send(&[register_ix(&ada.pubkey(), "vanessa")], &[&ada])
        .unwrap();
    let r = env.record("vanessa").unwrap();
    assert_eq!(r.name, "vanessa");
    assert_eq!(r.owner, ada.pubkey());
    assert_eq!(r.records, Records::default());
    assert_eq!(r.registered_at, r.updated_at);
}

#[test]
fn a_name_can_only_be_registered_once() {
    let mut env = ready();
    let (ada, bola) = (env.user(), env.user());
    env.send(&[register_ix(&ada.pubkey(), "lagos")], &[&ada])
        .unwrap();
    let again = env.send(&[register_ix(&bola.pubkey(), "lagos")], &[&bola]);
    assert!(again.unwrap_err().contains("already in use"));
    assert_eq!(env.record("lagos").unwrap().owner, ada.pubkey());
}

#[test]
fn refuses_names_that_break_the_rules() {
    let mut env = ready();
    let ada = env.user();
    let long = "a".repeat(33);
    let cases: &[(&str, &str)] = &[
        ("ab", "NameTooShort"),
        ("Vanessa", "NameBadCharacter"),
        ("van_essa", "NameBadCharacter"),
        ("van essa", "NameBadCharacter"),
        ("ada.beryl", "NameBadCharacter"),
        ("-ada", "NameEdgeHyphen"),
        ("ada-", "NameEdgeHyphen"),
    ];
    for (name, error) in cases {
        assert_error(
            env.send(&[register_ix(&ada.pubkey(), name)], &[&ada]),
            error,
        );
    }
    // 33 bytes can't even be a PDA seed: refused before the program runs, and never created.
    assert!(env
        .send(
            &[register_ix_at(&ada.pubkey(), &long, Pubkey::new_unique())],
            &[&ada]
        )
        .is_err());
}

#[test]
fn reserved_names_are_only_for_the_authority() {
    let mut env = ready();
    let ada = env.user();
    for name in ["admin", "beryl", "beryva", "support"] {
        assert_error(
            env.send(&[register_ix(&ada.pubkey(), name)], &[&ada]),
            "ReservedName",
        );
    }
    let auth = env.authority.insecure_clone();
    env.send(&[register_ix(&auth.pubkey(), "support")], &[&auth])
        .unwrap();
    assert_eq!(env.record("support").unwrap().owner, auth.pubkey());
}

#[test]
fn the_record_must_be_the_pda_of_that_name() {
    let mut env = ready();
    let ada = env.user();
    // The PDA of another name, or a random address: the seeds check refuses both.
    assert_error(
        env.send(
            &[register_ix_at(&ada.pubkey(), "ada", name_pda("bola"))],
            &[&ada],
        ),
        "ConstraintSeeds",
    );
    assert_error(
        env.send(
            &[register_ix_at(&ada.pubkey(), "ada", Pubkey::new_unique())],
            &[&ada],
        ),
        "ConstraintSeeds",
    );
    assert!(env.record("ada").is_none());
}

// ---------------------------------------------------------------------------
// transfer, records, primary, release

#[test]
fn the_owner_transfers_and_the_records_are_cleared() {
    let mut env = ready();
    let (ada, bola, eve) = (env.user(), env.user(), env.user());
    env.send(
        &[
            register_ix(&ada.pubkey(), "ada"),
            update_ix(&ada.pubkey(), "ada", records()),
        ],
        &[&ada],
    )
    .unwrap();
    assert_error(
        env.send(&[transfer_ix(&eve.pubkey(), "ada", eve.pubkey())], &[&eve]),
        "NotOwner",
    );
    assert_error(
        env.send(&[transfer_ix(&ada.pubkey(), "ada", ada.pubkey())], &[&ada]),
        "SameOwner",
    );
    assert_error(
        env.send(
            &[transfer_ix(&ada.pubkey(), "ada", Pubkey::default())],
            &[&ada],
        ),
        "InvalidNewOwner",
    );
    env.send(&[transfer_ix(&ada.pubkey(), "ada", bola.pubkey())], &[&ada])
        .unwrap();
    let r = env.record("ada").unwrap();
    assert_eq!(r.owner, bola.pubkey());
    assert_eq!(
        r.records,
        Records::default(),
        "records must not keep pointing at the old owner"
    );
    // The old owner can't act on it any more.
    assert_error(
        env.send(&[update_ix(&ada.pubkey(), "ada", records())], &[&ada]),
        "NotOwner",
    );
}

#[test]
fn the_owner_sets_records_within_the_rules() {
    let mut env = ready();
    let (ada, eve) = (env.user(), env.user());
    env.send(&[register_ix(&ada.pubkey(), "ada")], &[&ada])
        .unwrap();
    env.send(&[update_ix(&ada.pubkey(), "ada", records())], &[&ada])
        .unwrap();
    assert_eq!(env.record("ada").unwrap().records, records());
    assert_error(
        env.send(
            &[update_ix(&eve.pubkey(), "ada", Records::default())],
            &[&eve],
        ),
        "NotOwner",
    );
    let bad_btc = Records {
        btc: Some("bc1-not-a-signet-address-at-all!!".into()),
        ..Records::default()
    };
    assert_error(
        env.send(&[update_ix(&ada.pubkey(), "ada", bad_btc)], &[&ada]),
        "InvalidBtcRecord",
    );
    let bad_uri = Records {
        avatar: Some("javascript:alert(1)".into()),
        ..Records::default()
    };
    assert_error(
        env.send(&[update_ix(&ada.pubkey(), "ada", bad_uri)], &[&ada]),
        "InvalidAvatarUri",
    );
    let http_uri = Records {
        avatar: Some("http://example.com/a.png".into()),
        ..Records::default()
    };
    assert_error(
        env.send(&[update_ix(&ada.pubkey(), "ada", http_uri)], &[&ada]),
        "InvalidAvatarUri",
    );
    // Clearing works.
    env.send(
        &[update_ix(&ada.pubkey(), "ada", Records::default())],
        &[&ada],
    )
    .unwrap();
    assert_eq!(env.record("ada").unwrap().records, Records::default());
    // A record account that isn't a name's PDA is refused.
    assert!(env
        .send(
            &[update_ix_at(
                &ada.pubkey(),
                Pubkey::new_unique(),
                Records::default()
            )],
            &[&ada]
        )
        .is_err());
}

#[test]
fn primary_names_point_at_the_wallets_own_name() {
    let mut env = ready();
    let (ada, eve) = (env.user(), env.user());
    env.send(
        &[
            register_ix(&ada.pubkey(), "ada"),
            register_ix(&ada.pubkey(), "ada-two"),
        ],
        &[&ada],
    )
    .unwrap();
    assert_error(
        env.send(&[set_primary_ix(&eve.pubkey(), "ada")], &[&eve]),
        "NotOwner",
    );
    env.send(&[set_primary_ix(&ada.pubkey(), "ada")], &[&ada])
        .unwrap();
    assert_eq!(env.primary(&ada.pubkey()).unwrap().name, "ada");
    // Switching overwrites the same account.
    env.send(&[set_primary_ix(&ada.pubkey(), "ada-two")], &[&ada])
        .unwrap();
    let p = env.primary(&ada.pubkey()).unwrap();
    assert_eq!((p.name.as_str(), p.wallet), ("ada-two", ada.pubkey()));
    let before = env.lamports(&ada.pubkey());
    env.send(&[clear_primary_ix(&ada.pubkey())], &[&ada])
        .unwrap();
    assert!(env.primary(&ada.pubkey()).is_none());
    assert!(
        env.lamports(&ada.pubkey()) > before - 10_000,
        "the rent comes back"
    );
    assert!(env
        .send(&[clear_primary_ix(&eve.pubkey())], &[&eve])
        .is_err());
}

#[test]
fn releasing_closes_the_name_and_its_primary_and_frees_the_name() {
    let mut env = ready();
    let (ada, bola, eve) = (env.user(), env.user(), env.user());
    env.send(
        &[
            register_ix(&ada.pubkey(), "ada"),
            set_primary_ix(&ada.pubkey(), "ada"),
        ],
        &[&ada],
    )
    .unwrap();
    assert_error(
        env.send(&[release_ix(&eve.pubkey(), "ada", false)], &[&eve]),
        "NotOwner",
    );
    let before = env.lamports(&ada.pubkey());
    env.send(&[release_ix(&ada.pubkey(), "ada", true)], &[&ada])
        .unwrap();
    assert!(env.record("ada").is_none());
    assert!(env.primary(&ada.pubkey()).is_none());
    assert!(env.lamports(&ada.pubkey()) > before, "both rents come back");
    // Someone else can now have it.
    env.send(&[register_ix(&bola.pubkey(), "ada")], &[&bola])
        .unwrap();
    assert_eq!(env.record("ada").unwrap().owner, bola.pubkey());
}

#[test]
fn release_refuses_a_primary_that_points_elsewhere() {
    let mut env = ready();
    let ada = env.user();
    env.send(
        &[
            register_ix(&ada.pubkey(), "ada"),
            register_ix(&ada.pubkey(), "ada-two"),
            set_primary_ix(&ada.pubkey(), "ada-two"),
        ],
        &[&ada],
    )
    .unwrap();
    assert_error(
        env.send(&[release_ix(&ada.pubkey(), "ada", true)], &[&ada]),
        "PrimaryMismatch",
    );
    // Without the primary it's fine, and the primary stays.
    env.send(&[release_ix(&ada.pubkey(), "ada", false)], &[&ada])
        .unwrap();
    assert_eq!(env.primary(&ada.pubkey()).unwrap().name, "ada-two");
}

#[test]
fn a_stale_primary_after_a_transfer_names_a_record_owned_by_someone_else() {
    // Resolvers must check the record's owner: this is the case they guard against.
    let mut env = ready();
    let (ada, bola) = (env.user(), env.user());
    env.send(
        &[
            register_ix(&ada.pubkey(), "ada"),
            set_primary_ix(&ada.pubkey(), "ada"),
        ],
        &[&ada],
    )
    .unwrap();
    env.send(&[transfer_ix(&ada.pubkey(), "ada", bola.pubkey())], &[&ada])
        .unwrap();
    assert_eq!(env.primary(&ada.pubkey()).unwrap().name, "ada");
    assert_eq!(env.record("ada").unwrap().owner, bola.pubkey());
}
