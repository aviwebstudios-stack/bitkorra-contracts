//! The config: setup by the upgrade authority only, the fee cap (2%), the
//! treasury, and the two-step owner and arbiter handovers.

mod common;

use anchor_lang::prelude::Pubkey;
use beryl_escrow::{ArbiterAccepted, FeeSet};
use common::*;
use solana_signer::Signer;

#[test]
fn only_the_upgrade_authority_initializes_once() {
    let mut env = fresh();
    let stranger = env.user();
    let (o, a, t, m) = (
        env.owner.pubkey(),
        env.arbiter.pubkey(),
        env.treasury.pubkey(),
        env.mint,
    );
    assert_error(
        env.send(
            &[initialize_ix(&stranger.pubkey(), o, a, t, m)],
            &[&stranger],
        ),
        "NotOwner",
    );
    let ua = env.upgrade_authority.insecure_clone();
    assert!(env
        .send(
            &[initialize_ix_with(
                &ua.pubkey(),
                o,
                a,
                t,
                m,
                Pubkey::new_unique()
            )],
            &[&ua]
        )
        .is_err());
    assert!(
        env.send(&[initialize_ix(&ua.pubkey(), o, a, t, t)], &[&ua])
            .is_err(),
        "not a mint"
    );
    assert_error(
        env.send(
            &[initialize_ix(&ua.pubkey(), o, Pubkey::default(), t, m)],
            &[&ua],
        ),
        "InvalidKey",
    );
    env.send(&[initialize_ix(&ua.pubkey(), o, a, t, m)], &[&ua])
        .unwrap();
    let c = env.config();
    assert_eq!(
        (c.owner, c.arbiter, c.treasury, c.mint, c.fee_bps),
        (o, a, t, m, 0)
    );
    assert!(env
        .send(
            &[initialize_ix(&ua.pubkey(), stranger.pubkey(), a, t, m)],
            &[&ua]
        )
        .is_err());
}

#[test]
fn the_fee_is_capped_at_2_percent_and_only_the_owner_sets_it() {
    let mut env = ready();
    let stranger = env.user();
    assert_error(
        env.send(&[set_fee_ix(&stranger.pubkey(), 10)], &[&stranger]),
        "NotOwner",
    );
    let owner = env.owner.insecure_clone();
    assert_error(
        env.send(&[set_fee_ix(&owner.pubkey(), 201)], &[&owner]),
        "FeeTooHigh",
    );
    let logs = env
        .send(&[set_fee_ix(&owner.pubkey(), 200)], &[&owner])
        .unwrap();
    assert_eq!(event::<FeeSet>(&logs).unwrap().new_bps, 200);
    assert_error(
        env.send(
            &[set_treasury_ix(&stranger.pubkey(), stranger.pubkey())],
            &[&stranger],
        ),
        "NotOwner",
    );
    assert_error(
        env.send(
            &[set_treasury_ix(&owner.pubkey(), Pubkey::default())],
            &[&owner],
        ),
        "InvalidKey",
    );
    env.send(
        &[set_treasury_ix(&owner.pubkey(), stranger.pubkey())],
        &[&owner],
    )
    .unwrap();
    assert_eq!(env.config().treasury, stranger.pubkey());
}

#[test]
fn the_owner_changes_hands_in_two_steps() {
    let mut env = ready();
    let owner = env.owner.insecure_clone();
    let next = env.user();
    assert_error(
        env.send(&[accept_owner_ix(&next.pubkey())], &[&next]),
        "NothingPending",
    );
    env.send(
        &[propose_owner_ix(&owner.pubkey(), next.pubkey())],
        &[&owner],
    )
    .unwrap();
    assert_eq!(env.config().owner, owner.pubkey());
    let stranger = env.user();
    assert_error(
        env.send(&[accept_owner_ix(&stranger.pubkey())], &[&stranger]),
        "NotPending",
    );
    env.send(&[accept_owner_ix(&next.pubkey())], &[&next])
        .unwrap();
    assert_eq!(env.config().owner, next.pubkey());
    assert_error(
        env.send(&[set_fee_ix(&owner.pubkey(), 1)], &[&owner]),
        "NotOwner",
    );
}

#[test]
fn the_arbiter_changes_in_two_steps_proposed_by_the_owner() {
    let mut env = ready();
    let owner = env.owner.insecure_clone();
    let old = env.arbiter.insecure_clone();
    let next = env.user();
    // The arbiter can't name its own successor; only the owner proposes.
    assert_error(
        env.send(&[propose_arbiter_ix(&old.pubkey(), next.pubkey())], &[&old]),
        "NotOwner",
    );
    env.send(
        &[propose_arbiter_ix(&owner.pubkey(), next.pubkey())],
        &[&owner],
    )
    .unwrap();
    assert_eq!(env.config().arbiter, old.pubkey());
    assert_error(
        env.send(&[accept_arbiter_ix(&owner.pubkey())], &[&owner]),
        "NotPending",
    );
    let logs = env
        .send(&[accept_arbiter_ix(&next.pubkey())], &[&next])
        .unwrap();
    let e: ArbiterAccepted = event(&logs).unwrap();
    assert_eq!((e.old, e.new), (old.pubkey(), next.pubkey()));
    assert_eq!(env.config().pending_arbiter, None);
    // Cancelling a proposal.
    env.send(
        &[propose_arbiter_ix(&owner.pubkey(), old.pubkey())],
        &[&owner],
    )
    .unwrap();
    env.send(
        &[propose_arbiter_ix(&owner.pubkey(), Pubkey::default())],
        &[&owner],
    )
    .unwrap();
    assert_error(
        env.send(&[accept_arbiter_ix(&old.pubkey())], &[&old]),
        "NothingPending",
    );
}
