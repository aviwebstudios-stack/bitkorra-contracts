//! Escrows in SOL and in the accepted token: funding, mark paid, release,
//! refund after the deadline, the buyer cancelling, disputes and the
//! arbiter, the fee, and every refusal.

mod common;

use anchor_lang::prelude::Pubkey;
use beryl_escrow::{
    state::{EscrowState, Outcome},
    EscrowFunded, EscrowSettled,
};
use common::*;
use solana_signer::Signer;

// ---------------------------------------------------------------------------
// Funding

#[test]
fn funding_locks_the_sol_in_the_escrow_account() {
    let mut env = ready();
    let (seller, buyer) = (env.user(), env.user());
    let deadline = env.now() + WINDOW;
    let s0 = env.lamports(&seller.pubkey());
    let logs = env
        .send(
            &[fund_sol_ix(
                &seller.pubkey(),
                order(1),
                buyer.pubkey(),
                SOL,
                deadline,
            )],
            &[&seller],
        )
        .unwrap();
    let key = escrow_pda(&seller.pubkey(), &order(1));
    let e = env.escrow(&key).unwrap();
    assert_eq!(
        (e.seller, e.buyer, e.amount, e.deadline, e.state, e.fee_bps),
        (
            seller.pubkey(),
            buyer.pubkey(),
            SOL,
            deadline,
            EscrowState::Funded,
            0
        )
    );
    assert!(env.lamports(&key) > SOL, "the amount plus rent");
    assert!(s0 - env.lamports(&seller.pubkey()) > SOL);
    let f: EscrowFunded = event(&logs).unwrap();
    assert_eq!((f.escrow, f.amount, f.mint), (key, SOL, None));
    // One escrow per seller and order.
    assert!(env
        .send(
            &[fund_sol_ix(
                &seller.pubkey(),
                order(1),
                buyer.pubkey(),
                SOL,
                deadline
            )],
            &[&seller]
        )
        .is_err());
}

#[test]
fn funding_refuses_bad_terms() {
    let mut env = ready();
    let (seller, buyer) = (env.user(), env.user());
    let now = env.now();
    let fund = |env: &mut Env, b: Pubkey, amount: u64, deadline: i64| {
        env.send(
            &[fund_sol_ix(&seller.pubkey(), order(2), b, amount, deadline)],
            &[&seller],
        )
    };
    assert_error(
        fund(&mut env, buyer.pubkey(), 0, now + WINDOW),
        "ZeroAmount",
    );
    assert_error(
        fund(&mut env, seller.pubkey(), SOL, now + WINDOW),
        "BadBuyer",
    );
    assert_error(
        fund(&mut env, Pubkey::default(), SOL, now + WINDOW),
        "BadBuyer",
    );
    assert_error(fund(&mut env, buyer.pubkey(), SOL, now), "BadDeadline");
    assert_error(
        fund(&mut env, buyer.pubkey(), SOL, now + 24 * 3600 + 1),
        "BadDeadline",
    );
    assert!(
        fund(&mut env, buyer.pubkey(), 1_000 * SOL, now + WINDOW).is_err(),
        "more than the seller has"
    );
}

#[test]
fn token_funding_checks_the_mint_and_the_sellers_account() {
    let mut env = ready();
    let (seller, buyer, other) = (env.user(), env.user(), env.user());
    let mine = env.token_account(&seller.pubkey(), 10 * USDC);
    let theirs = env.token_account(&other.pubkey(), 10 * USDC);
    let deadline = env.now() + WINDOW;
    let mint = env.mint;
    // Someone else's tokens: refused.
    assert!(env
        .send(
            &[fund_token_ix(
                &seller.pubkey(),
                theirs,
                mint,
                order(3),
                buyer.pubkey(),
                USDC,
                deadline
            )],
            &[&seller]
        )
        .is_err());
    // Another mint: refused.
    let fake_mint = Pubkey::new_unique();
    assert!(env
        .send(
            &[fund_token_ix(
                &seller.pubkey(),
                mine,
                fake_mint,
                order(3),
                buyer.pubkey(),
                USDC,
                deadline
            )],
            &[&seller]
        )
        .is_err());
    env.send(
        &[fund_token_ix(
            &seller.pubkey(),
            mine,
            mint,
            order(3),
            buyer.pubkey(),
            4 * USDC,
            deadline,
        )],
        &[&seller],
    )
    .unwrap();
    let key = escrow_pda(&seller.pubkey(), &order(3));
    assert_eq!(env.tokens(&vault_pda(&key)), 4 * USDC);
    assert_eq!(env.tokens(&mine), 6 * USDC);
}

// ---------------------------------------------------------------------------
// The happy path and the fee

#[test]
fn the_seller_releases_to_the_buyer_and_the_escrow_closes_with_rent_back() {
    let mut env = ready();
    let (seller, buyer, s) = sol_escrow(&mut env, SOL);
    env.send(&[mark_paid_ix(&buyer.pubkey(), &s.escrow)], &[&buyer])
        .unwrap();
    let (b0, s0, rent) = (
        env.lamports(&buyer.pubkey()),
        env.lamports(&seller.pubkey()),
        env.lamports(&s.escrow) - SOL,
    );
    let logs = env
        .send(&[release_ix(&seller.pubkey(), &s)], &[&seller])
        .unwrap();
    assert_eq!(env.lamports(&buyer.pubkey()) - b0, SOL);
    assert_eq!(
        env.lamports(&seller.pubkey()) + 5_000 - s0,
        rent,
        "the rent comes back"
    );
    assert!(!env.exists(&s.escrow));
    let e: EscrowSettled = event(&logs).unwrap();
    assert_eq!(
        (e.outcome, e.to_buyer, e.to_seller, e.fee),
        (Outcome::Released, SOL, 0, 0)
    );
    // No double release.
    assert!(env
        .send(&[release_ix(&seller.pubkey(), &s)], &[&seller])
        .is_err());
}

#[test]
fn a_release_pays_the_fee_fixed_at_funding_and_only_to_the_treasury() {
    let mut env = ready_with_fee(150);
    let (seller, _buyer, s) = sol_escrow(&mut env, 1_000_003);
    // A later fee change doesn't touch an open escrow.
    let owner = env.owner.insecure_clone();
    env.send(&[set_fee_ix(&owner.pubkey(), 200)], &[&owner])
        .unwrap();
    let (b0, t0) = (env.lamports(&s.buyer), env.lamports(&env.treasury.pubkey()));
    // Without the treasury account, a release with a fee is refused.
    let mut no_treasury = s.clone();
    no_treasury.treasury = None;
    assert_error(
        env.send(&[release_ix(&seller.pubkey(), &no_treasury)], &[&seller]),
        "WrongAccount",
    );
    let mut fake = s.clone();
    fake.treasury = Some(Pubkey::new_unique());
    assert_error(
        env.send(&[release_ix(&seller.pubkey(), &fake)], &[&seller]),
        "WrongAccount",
    );
    env.send(&[release_ix(&seller.pubkey(), &s)], &[&seller])
        .unwrap();
    // 1.5% of 1,000,003 = 15,000.045: 15,000 to the treasury.
    assert_eq!(env.lamports(&env.treasury.pubkey()) - t0, 15_000);
    assert_eq!(env.lamports(&s.buyer) - b0, 985_003);
}

#[test]
fn only_the_seller_releases_and_money_only_goes_to_this_escrows_parties() {
    let mut env = ready();
    let (_seller, buyer, s) = sol_escrow(&mut env, SOL);
    let stranger = env.user();
    assert_error(
        env.send(&[release_ix(&buyer.pubkey(), &s)], &[&buyer]),
        "NotSeller",
    );
    assert_error(
        env.send(&[release_ix(&stranger.pubkey(), &s)], &[&stranger]),
        "NotSeller",
    );
    // Swapping in another account as the buyer or seller: refused.
    let mut thief = s.clone();
    thief.buyer = stranger.pubkey();
    assert_error(
        env.send(&[release_ix(&stranger.pubkey(), &thief)], &[&stranger]),
        "WrongAccount",
    );
    let mut thief = s.clone();
    thief.seller = stranger.pubkey();
    assert!(env
        .send(&[release_ix(&stranger.pubkey(), &thief)], &[&stranger])
        .is_err());
    assert!(env.escrow(&s.escrow).is_some());
}

// ---------------------------------------------------------------------------
// Paid, deadline, refund and cancel

#[test]
fn only_the_buyer_marks_paid_and_only_before_the_deadline() {
    let mut env = ready();
    let (seller, buyer, s) = sol_escrow(&mut env, SOL);
    assert_error(
        env.send(&[mark_paid_ix(&seller.pubkey(), &s.escrow)], &[&seller]),
        "NotBuyer",
    );
    let t = env.now();
    env.set_time(t + WINDOW + 1);
    assert_error(
        env.send(&[mark_paid_ix(&buyer.pubkey(), &s.escrow)], &[&buyer]),
        "DeadlinePassed",
    );
    env.set_time(t + WINDOW);
    env.send(&[mark_paid_ix(&buyer.pubkey(), &s.escrow)], &[&buyer])
        .unwrap();
    assert_eq!(env.escrow(&s.escrow).unwrap().state, EscrowState::Paid);
    assert_error(
        env.send(&[mark_paid_ix(&buyer.pubkey(), &s.escrow)], &[&buyer]),
        "WrongState",
    );
}

#[test]
fn the_seller_reclaims_only_after_the_deadline_and_only_if_never_marked_paid() {
    let mut env = ready();
    let (seller, buyer, s) = sol_escrow(&mut env, SOL);
    assert_error(
        env.send(&[refund_ix(&seller.pubkey(), &s)], &[&seller]),
        "DeadlineNotPassed",
    );
    let t = env.now();
    env.set_time(t + WINDOW + 1);
    assert_error(
        env.send(&[refund_ix(&buyer.pubkey(), &s)], &[&buyer]),
        "NotSeller",
    );
    let s0 = env.lamports(&seller.pubkey());
    let logs = env
        .send(&[refund_ix(&seller.pubkey(), &s)], &[&seller])
        .unwrap();
    assert!(
        env.lamports(&seller.pubkey()) + 5_000 - s0 > SOL,
        "the amount and the rent"
    );
    assert_eq!(
        event::<EscrowSettled>(&logs).unwrap().outcome,
        Outcome::Refunded
    );
    // Once marked paid, the deadline doesn't let the seller take it back.
    let mut env = ready();
    let (seller, buyer, s) = sol_escrow(&mut env, SOL);
    env.send(&[mark_paid_ix(&buyer.pubkey(), &s.escrow)], &[&buyer])
        .unwrap();
    let t = env.now();
    env.set_time(t + 10 * WINDOW);
    assert_error(
        env.send(&[refund_ix(&seller.pubkey(), &s)], &[&seller]),
        "WrongState",
    );
}

#[test]
fn the_buyer_can_cancel_before_paying_and_it_goes_back_to_the_seller() {
    let mut env = ready();
    let (seller, buyer, s) = sol_escrow(&mut env, SOL);
    assert_error(
        env.send(&[cancel_ix(&seller.pubkey(), &s)], &[&seller]),
        "NotBuyer",
    );
    let s0 = env.lamports(&seller.pubkey());
    let logs = env
        .send(&[cancel_ix(&buyer.pubkey(), &s)], &[&buyer])
        .unwrap();
    assert!(env.lamports(&seller.pubkey()) - s0 > SOL);
    assert_eq!(
        event::<EscrowSettled>(&logs).unwrap().outcome,
        Outcome::Cancelled
    );
    let mut env = ready();
    let (_seller, buyer, s) = sol_escrow(&mut env, SOL);
    env.send(&[mark_paid_ix(&buyer.pubkey(), &s.escrow)], &[&buyer])
        .unwrap();
    assert_error(
        env.send(&[cancel_ix(&buyer.pubkey(), &s)], &[&buyer]),
        "WrongState",
    );
}

// ---------------------------------------------------------------------------
// Disputes

#[test]
fn a_dispute_opens_only_after_paid_and_only_by_a_party() {
    let mut env = ready();
    let (seller, buyer, s) = sol_escrow(&mut env, SOL);
    assert_error(
        env.send(&[open_dispute_ix(&buyer.pubkey(), &s.escrow)], &[&buyer]),
        "WrongState",
    );
    env.send(&[mark_paid_ix(&buyer.pubkey(), &s.escrow)], &[&buyer])
        .unwrap();
    let stranger = env.user();
    assert_error(
        env.send(
            &[open_dispute_ix(&stranger.pubkey(), &s.escrow)],
            &[&stranger],
        ),
        "NotParty",
    );
    env.send(&[open_dispute_ix(&seller.pubkey(), &s.escrow)], &[&seller])
        .unwrap();
    assert_eq!(env.escrow(&s.escrow).unwrap().state, EscrowState::Disputed);
    // The seller can't reclaim, the buyer can't cancel; the seller can still release.
    let t = env.now();
    env.set_time(t + 10 * WINDOW);
    assert_error(
        env.send(&[refund_ix(&seller.pubkey(), &s)], &[&seller]),
        "WrongState",
    );
    assert_error(
        env.send(&[cancel_ix(&buyer.pubkey(), &s)], &[&buyer]),
        "WrongState",
    );
    env.send(&[release_ix(&seller.pubkey(), &s)], &[&seller])
        .unwrap();
}

#[test]
fn only_the_arbiter_resolves_and_only_a_disputed_escrow() {
    for to_buyer in [true, false] {
        let mut env = ready_with_fee(100);
        let (seller, buyer, s) = sol_escrow(&mut env, SOL);
        let arbiter = env.arbiter.insecure_clone();
        env.send(&[mark_paid_ix(&buyer.pubkey(), &s.escrow)], &[&buyer])
            .unwrap();
        assert_error(
            env.send(&[resolve_ix(&arbiter.pubkey(), &s, to_buyer)], &[&arbiter]),
            "WrongState",
        );
        env.send(&[open_dispute_ix(&buyer.pubkey(), &s.escrow)], &[&buyer])
            .unwrap();
        for p in [&seller, &buyer, &env.owner.insecure_clone()] {
            assert_error(
                env.send(&[resolve_ix(&p.pubkey(), &s, to_buyer)], &[p]),
                "NotArbiter",
            );
        }
        let (b0, s0, t0) = (
            env.lamports(&buyer.pubkey()),
            env.lamports(&seller.pubkey()),
            env.lamports(&env.treasury.pubkey()),
        );
        let logs = env
            .send(&[resolve_ix(&arbiter.pubkey(), &s, to_buyer)], &[&arbiter])
            .unwrap();
        let e: EscrowSettled = event(&logs).unwrap();
        if to_buyer {
            assert_eq!(e.outcome, Outcome::ResolvedBuyer);
            assert_eq!(env.lamports(&buyer.pubkey()) - b0, SOL - SOL / 100);
            assert_eq!(env.lamports(&env.treasury.pubkey()) - t0, SOL / 100);
        } else {
            assert_eq!(e.outcome, Outcome::ResolvedSeller);
            assert!(
                env.lamports(&seller.pubkey()) - s0 > SOL,
                "a refund pays no fee"
            );
            assert_eq!(env.lamports(&env.treasury.pubkey()), t0);
        }
        assert!(!env.exists(&s.escrow));
    }
}

// ---------------------------------------------------------------------------
// Tokens

#[test]
fn token_escrows_release_refund_and_resolve_and_close_the_vault() {
    // Release with a fee.
    let mut env = ready_with_fee(200);
    let (seller, buyer, s) = token_escrow(&mut env, 10 * USDC);
    let (bt, st, tt, _) = s.token.unwrap();
    env.send(&[mark_paid_ix(&buyer.pubkey(), &s.escrow)], &[&buyer])
        .unwrap();
    env.send(&[release_ix(&seller.pubkey(), &s)], &[&seller])
        .unwrap();
    assert_eq!(env.tokens(&bt), 10 * USDC - 200_000);
    assert_eq!(env.tokens(&tt.unwrap()), 200_000);
    assert!(!env.exists(&vault_pda(&s.escrow)) && !env.exists(&s.escrow));
    // Refund after the deadline: everything back.
    let mut env = ready_with_fee(200);
    let (seller, _buyer, s) = token_escrow(&mut env, 10 * USDC);
    let (_, st2, _, _) = s.token.unwrap();
    let t = env.now();
    env.set_time(t + WINDOW + 1);
    env.send(&[refund_ix(&seller.pubkey(), &s)], &[&seller])
        .unwrap();
    assert_eq!(env.tokens(&st2), 100 * USDC);
    let _ = st;
    // Resolved for the seller.
    let mut env = ready();
    let (seller, buyer, s) = token_escrow(&mut env, 5 * USDC);
    let arbiter = env.arbiter.insecure_clone();
    env.send(&[mark_paid_ix(&buyer.pubkey(), &s.escrow)], &[&buyer])
        .unwrap();
    env.send(&[open_dispute_ix(&buyer.pubkey(), &s.escrow)], &[&buyer])
        .unwrap();
    env.send(&[resolve_ix(&arbiter.pubkey(), &s, false)], &[&arbiter])
        .unwrap();
    assert_eq!(env.tokens(&s.token.unwrap().1), 100 * USDC);
    let _ = seller;
}

#[test]
fn token_settlements_check_every_account() {
    let mut env = ready_with_fee(100);
    let (seller, _buyer, s) = token_escrow(&mut env, 10 * USDC);
    let (bt, st, tt, mint) = s.token.unwrap();
    let stranger = env.user();
    let strangers = env.token_account(&stranger.pubkey(), 0);
    let try_with = |env: &mut Env, token: Option<(Pubkey, Pubkey, Option<Pubkey>, Pubkey)>| {
        let mut x = s.clone();
        x.token = token;
        env.send(&[release_ix(&seller.pubkey(), &x)], &[&seller])
    };
    assert_error(try_with(&mut env, None), "TokenAccountsMissing");
    assert_error(
        try_with(&mut env, Some((strangers, st, tt, mint))),
        "WrongAccount",
    );
    assert_error(
        try_with(&mut env, Some((bt, st, Some(strangers), mint))),
        "WrongAccount",
    );
    assert_error(
        try_with(&mut env, Some((bt, st, None, mint))),
        "TokenAccountsMissing",
    );
    assert!(
        try_with(&mut env, Some((bt, st, tt, Pubkey::new_unique()))).is_err(),
        "another mint"
    );
    // A fake vault (any token account at another address) is refused.
    let mut fake = s.clone();
    fake.escrow = s.escrow;
    let ix = {
        let mut ix = release_ix(&seller.pubkey(), &fake);
        ix.accounts[6].pubkey = strangers;
        ix
    };
    assert_error(env.send(&[ix], &[&seller]), "WrongAccount");
    assert_eq!(
        env.tokens(&vault_pda(&s.escrow)),
        10 * USDC,
        "nothing moved"
    );
}
