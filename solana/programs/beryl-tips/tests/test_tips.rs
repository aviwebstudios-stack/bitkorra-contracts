//! Tips in SOL and in the accepted token: paid straight to the creator in
//! the same transaction, the fee split, and every refusal.

mod common;

use beryl_tips::TipSent;
use common::*;
use solana_signer::Signer;

#[test]
fn a_sol_tip_goes_straight_to_the_creator() {
    let mut env = ready();
    let (tipper, creator) = (env.user(), env.user());
    let treasury = env.treasury.pubkey();
    let (c0, t0, k0) = (
        env.lamports(&creator.pubkey()),
        env.lamports(&treasury),
        env.lamports(&config_pda()),
    );
    let logs = env
        .send(
            &[tip_sol_ix(
                &tipper.pubkey(),
                &creator.pubkey(),
                &treasury,
                SOL / 10,
                reference(7),
            )],
            &[&tipper],
        )
        .unwrap();
    assert_eq!(env.lamports(&creator.pubkey()) - c0, SOL / 10);
    assert_eq!(env.lamports(&treasury), t0, "no fee by default");
    // The program never holds the money.
    assert_eq!(env.lamports(&config_pda()), k0);
    let e: TipSent = event(&logs).expect("TipSent");
    assert_eq!(e.tipper, tipper.pubkey());
    assert_eq!(e.creator, creator.pubkey());
    assert_eq!(
        (e.mint, e.amount, e.fee, e.reference),
        (None, SOL / 10, 0, reference(7))
    );
}

#[test]
fn with_a_fee_the_treasury_gets_its_share_and_the_creator_the_rest() {
    let mut env = ready_with_fee(250);
    let (tipper, creator) = (env.user(), env.user());
    let treasury = env.treasury.pubkey();
    let (c0, t0, p0) = (
        env.lamports(&creator.pubkey()),
        env.lamports(&treasury),
        env.lamports(&tipper.pubkey()),
    );
    let logs = env
        .send(
            &[tip_sol_ix(
                &tipper.pubkey(),
                &creator.pubkey(),
                &treasury,
                1_000_003,
                reference(1),
            )],
            &[&tipper],
        )
        .unwrap();
    // 2.5% of 1,000,003 = 25,000.075, rounded down for the creator.
    assert_eq!(env.lamports(&treasury) - t0, 25_000);
    assert_eq!(env.lamports(&creator.pubkey()) - c0, 975_003);
    // The tipper paid the amount and the network fee (5,000 per signature), nothing else.
    assert_eq!(p0 - env.lamports(&tipper.pubkey()), 1_000_003 + 5_000);
    let e: TipSent = event(&logs).unwrap();
    assert_eq!((e.amount, e.fee), (1_000_003, 25_000));
}

#[test]
fn sol_tips_refuse_zero_self_wrong_treasury_and_program_accounts() {
    let mut env = ready();
    let (tipper, creator) = (env.user(), env.user());
    let treasury = env.treasury.pubkey();
    assert_error(
        env.send(
            &[tip_sol_ix(
                &tipper.pubkey(),
                &creator.pubkey(),
                &treasury,
                0,
                reference(1),
            )],
            &[&tipper],
        ),
        "ZeroAmount",
    );
    assert_error(
        env.send(
            &[tip_sol_ix(
                &tipper.pubkey(),
                &tipper.pubkey(),
                &treasury,
                SOL,
                reference(1),
            )],
            &[&tipper],
        ),
        "SelfTip",
    );
    let fake_treasury = env.user();
    assert_error(
        env.send(
            &[tip_sol_ix(
                &tipper.pubkey(),
                &creator.pubkey(),
                &fake_treasury.pubkey(),
                SOL,
                reference(1),
            )],
            &[&tipper],
        ),
        "WrongTreasury",
    );
    // The creator must be a wallet: a program-owned account (here the config) is refused.
    assert!(env
        .send(
            &[tip_sol_ix(
                &tipper.pubkey(),
                &config_pda(),
                &treasury,
                SOL,
                reference(1)
            )],
            &[&tipper]
        )
        .is_err());
    // More than the tipper has fails as a whole.
    assert!(env
        .send(
            &[tip_sol_ix(
                &tipper.pubkey(),
                &creator.pubkey(),
                &treasury,
                1_000 * SOL,
                reference(1)
            )],
            &[&tipper]
        )
        .is_err());
}

#[test]
fn a_sol_tip_must_be_signed_by_the_tipper() {
    let mut env = ready();
    let (tipper, creator, payer) = (env.user(), env.user(), env.user());
    let treasury = env.treasury.pubkey();
    let mut ix = tip_sol_ix(
        &tipper.pubkey(),
        &creator.pubkey(),
        &treasury,
        SOL,
        reference(1),
    );
    ix.accounts[0].is_signer = false;
    assert!(env.send(&[ix], &[&payer]).is_err());
}

#[test]
fn a_token_tip_goes_straight_to_the_creator() {
    let mut env = ready();
    let (tipper, creator) = (env.user(), env.user());
    let payer_token = env.token_account(&tipper.pubkey(), 10 * USDC);
    let creator_token = env.token_account(&creator.pubkey(), 0);
    let t = TokenAccounts {
        payer_token,
        creator: creator.pubkey(),
        creator_token,
        mint: env.mint,
        treasury_token: None,
    };
    let logs = env
        .send(
            &[tip_token_ix(&tipper.pubkey(), &t, 2 * USDC, reference(9))],
            &[&tipper],
        )
        .unwrap();
    assert_eq!(env.tokens(&creator_token), 2 * USDC);
    assert_eq!(env.tokens(&payer_token), 8 * USDC);
    let e: TipSent = event(&logs).unwrap();
    assert_eq!((e.mint, e.amount, e.fee), (Some(env.mint), 2 * USDC, 0));
}

#[test]
fn a_token_tip_with_a_fee_needs_the_treasury_token_account() {
    let mut env = ready_with_fee(500);
    let (tipper, creator) = (env.user(), env.user());
    let treasury = env.treasury.pubkey();
    let payer_token = env.token_account(&tipper.pubkey(), 10 * USDC);
    let creator_token = env.token_account(&creator.pubkey(), 0);
    let treasury_token = env.token_account(&treasury, 0);
    let mut t = TokenAccounts {
        payer_token,
        creator: creator.pubkey(),
        creator_token,
        mint: env.mint,
        treasury_token: None,
    };
    assert_error(
        env.send(
            &[tip_token_ix(&tipper.pubkey(), &t, USDC, reference(1))],
            &[&tipper],
        ),
        "TreasuryAccountMissing",
    );
    // A token account that isn't the treasury's is refused.
    let impostor = env.user();
    t.treasury_token = Some(env.token_account(&impostor.pubkey(), 0));
    assert_error(
        env.send(
            &[tip_token_ix(&tipper.pubkey(), &t, USDC, reference(1))],
            &[&tipper],
        ),
        "WrongTreasury",
    );
    t.treasury_token = Some(treasury_token);
    env.send(
        &[tip_token_ix(&tipper.pubkey(), &t, USDC, reference(1))],
        &[&tipper],
    )
    .unwrap();
    assert_eq!(env.tokens(&treasury_token), USDC / 20);
    assert_eq!(env.tokens(&creator_token), USDC - USDC / 20);
    assert_eq!(env.tokens(&payer_token), 9 * USDC);
}

#[test]
fn token_tips_check_the_mint_and_every_owner() {
    let mut env = ready();
    let (tipper, creator, other) = (env.user(), env.user(), env.user());
    let payer_token = env.token_account(&tipper.pubkey(), 10 * USDC);
    let creator_token = env.token_account(&creator.pubkey(), 0);
    let others_token = env.token_account(&other.pubkey(), 10 * USDC);
    let mint = env.mint;
    let base = || TokenAccounts {
        payer_token,
        creator: creator.pubkey(),
        creator_token,
        mint,
        treasury_token: None,
    };
    // Someone else's token account as the destination: refused (it must be the creator's).
    let mut t = base();
    t.creator_token = others_token;
    assert!(env
        .send(
            &[tip_token_ix(&tipper.pubkey(), &t, USDC, reference(1))],
            &[&tipper]
        )
        .is_err());
    // Paying from a token account the tipper doesn't own: refused.
    let mut t = base();
    t.payer_token = others_token;
    assert!(env
        .send(
            &[tip_token_ix(&tipper.pubkey(), &t, USDC, reference(1))],
            &[&tipper]
        )
        .is_err());
    assert_eq!(env.tokens(&others_token), 10 * USDC);
    // Another mint entirely: refused.
    let (other_mint, other_ata) = env.other_mint_account(&tipper.pubkey(), 10 * USDC);
    let mut t = base();
    t.mint = other_mint;
    t.payer_token = other_ata;
    assert_error(
        env.send(
            &[tip_token_ix(&tipper.pubkey(), &t, USDC, reference(1))],
            &[&tipper],
        ),
        "WrongMint",
    );
    // Zero, self, too much.
    assert_error(
        env.send(
            &[tip_token_ix(&tipper.pubkey(), &base(), 0, reference(1))],
            &[&tipper],
        ),
        "ZeroAmount",
    );
    let mut t = base();
    t.creator = tipper.pubkey();
    // (A second token account of the tipper, so it's the program's own check that refuses.)
    t.creator_token = env.extra_token_account(&tipper.pubkey());
    assert_error(
        env.send(
            &[tip_token_ix(&tipper.pubkey(), &t, USDC, reference(1))],
            &[&tipper],
        ),
        "SelfTip",
    );
    assert!(env
        .send(
            &[tip_token_ix(
                &tipper.pubkey(),
                &base(),
                11 * USDC,
                reference(1)
            )],
            &[&tipper]
        )
        .is_err());
    assert_eq!(env.tokens(&creator_token), 0);
    assert_eq!(env.tokens(&payer_token), 10 * USDC);
}
