//! The config: who can set it up, the fee and its cap, the treasury, and the
//! two-step authority handover.

mod common;

use anchor_lang::prelude::Pubkey;
use beryl_tips::{AuthorityAccepted, FeeSet};
use common::*;
use solana_signer::Signer;

#[test]
fn only_the_upgrade_authority_can_initialize_and_only_once() {
    let mut env = fresh();
    let stranger = env.user();
    let (authority, treasury, mint) = (env.authority.pubkey(), env.treasury.pubkey(), env.mint);
    assert_error(
        env.send(
            &[initialize_ix(&stranger.pubkey(), authority, treasury, mint)],
            &[&stranger],
        ),
        "NotAuthority",
    );
    // A ProgramData account that isn't this program's is refused.
    let ua = env.upgrade_authority.insecure_clone();
    assert!(env
        .send(
            &[initialize_ix_with(
                &ua.pubkey(),
                authority,
                treasury,
                mint,
                Pubkey::new_unique()
            )],
            &[&ua]
        )
        .is_err());
    // The mint must be a real SPL Token mint.
    assert!(env
        .send(
            &[initialize_ix(&ua.pubkey(), authority, treasury, treasury)],
            &[&ua]
        )
        .is_err());
    // No zero keys.
    assert_error(
        env.send(
            &[initialize_ix(
                &ua.pubkey(),
                authority,
                Pubkey::default(),
                mint,
            )],
            &[&ua],
        ),
        "InvalidKey",
    );
    env.send(
        &[initialize_ix(&ua.pubkey(), authority, treasury, mint)],
        &[&ua],
    )
    .unwrap();
    let c = env.config();
    assert_eq!(
        (c.authority, c.treasury, c.mint, c.fee_bps),
        (authority, treasury, mint, 0)
    );
    assert_eq!(c.pending_authority, None);
    // Once: the config account already exists.
    assert!(env
        .send(
            &[initialize_ix(
                &ua.pubkey(),
                stranger.pubkey(),
                treasury,
                mint
            )],
            &[&ua]
        )
        .is_err());
    assert_eq!(env.config().authority, authority);
}

#[test]
fn the_fee_is_capped_at_5_percent_and_only_the_authority_sets_it() {
    let mut env = ready();
    let stranger = env.user();
    assert_error(
        env.send(&[set_fee_ix(&stranger.pubkey(), 100)], &[&stranger]),
        "NotAuthority",
    );
    let auth = env.authority.insecure_clone();
    assert_error(
        env.send(&[set_fee_ix(&auth.pubkey(), 501)], &[&auth]),
        "FeeTooHigh",
    );
    assert_error(
        env.send(&[set_fee_ix(&auth.pubkey(), u16::MAX)], &[&auth]),
        "FeeTooHigh",
    );
    let logs = env
        .send(&[set_fee_ix(&auth.pubkey(), 500)], &[&auth])
        .unwrap();
    assert_eq!(env.config().fee_bps, 500);
    let e: FeeSet = event(&logs).expect("FeeSet");
    assert_eq!((e.old_bps, e.new_bps), (0, 500));
    env.send(&[set_fee_ix(&auth.pubkey(), 0)], &[&auth])
        .unwrap();
    assert_eq!(env.config().fee_bps, 0);
}

#[test]
fn only_the_authority_moves_the_treasury() {
    let mut env = ready();
    let stranger = env.user();
    let new_treasury = Pubkey::new_unique();
    assert_error(
        env.send(
            &[set_treasury_ix(&stranger.pubkey(), new_treasury)],
            &[&stranger],
        ),
        "NotAuthority",
    );
    let auth = env.authority.insecure_clone();
    assert_error(
        env.send(
            &[set_treasury_ix(&auth.pubkey(), Pubkey::default())],
            &[&auth],
        ),
        "InvalidKey",
    );
    env.send(&[set_treasury_ix(&auth.pubkey(), new_treasury)], &[&auth])
        .unwrap();
    assert_eq!(env.config().treasury, new_treasury);
}

#[test]
fn the_authority_changes_hands_in_two_steps() {
    let mut env = ready();
    let auth = env.authority.insecure_clone();
    let next = env.user();
    let stranger = env.user();
    // Nothing proposed yet.
    assert_error(
        env.send(&[accept_authority_ix(&next.pubkey())], &[&next]),
        "NoPendingAuthority",
    );
    // Only the authority proposes.
    assert_error(
        env.send(
            &[propose_authority_ix(&stranger.pubkey(), stranger.pubkey())],
            &[&stranger],
        ),
        "NotAuthority",
    );
    env.send(
        &[propose_authority_ix(&auth.pubkey(), next.pubkey())],
        &[&auth],
    )
    .unwrap();
    assert_eq!(env.config().pending_authority, Some(next.pubkey()));
    // Proposing alone changes nothing; only the proposed key can accept.
    assert_eq!(env.config().authority, auth.pubkey());
    assert_error(
        env.send(&[accept_authority_ix(&stranger.pubkey())], &[&stranger]),
        "NotPendingAuthority",
    );
    let logs = env
        .send(&[accept_authority_ix(&next.pubkey())], &[&next])
        .unwrap();
    let e: AuthorityAccepted = event(&logs).expect("AuthorityAccepted");
    assert_eq!((e.old, e.new), (auth.pubkey(), next.pubkey()));
    let c = env.config();
    assert_eq!((c.authority, c.pending_authority), (next.pubkey(), None));
    // The old authority has no say any more.
    assert_error(
        env.send(&[set_fee_ix(&auth.pubkey(), 1)], &[&auth]),
        "NotAuthority",
    );
}

#[test]
fn a_proposal_can_be_cancelled() {
    let mut env = ready();
    let auth = env.authority.insecure_clone();
    let next = env.user();
    env.send(
        &[propose_authority_ix(&auth.pubkey(), next.pubkey())],
        &[&auth],
    )
    .unwrap();
    env.send(
        &[propose_authority_ix(&auth.pubkey(), Pubkey::default())],
        &[&auth],
    )
    .unwrap();
    assert_eq!(env.config().pending_authority, None);
    assert_error(
        env.send(&[accept_authority_ix(&next.pubkey())], &[&next]),
        "NoPendingAuthority",
    );
}
