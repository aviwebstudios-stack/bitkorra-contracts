//! proptest on amounts and fees: the split's rules on their own, then real
//! tips and purchases in LiteSVM with random amounts and fees.

mod common;

use beryl_tips::{
    fees::{split, BPS_DENOMINATOR, MAX_FEE_BPS},
    state::Currency,
};
use common::*;
use proptest::prelude::*;
use solana_signer::Signer;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(5_000))]

    #[test]
    fn the_split_always_adds_up_and_never_passes_the_cap(amount in any::<u64>(), bps in 0u16..=MAX_FEE_BPS) {
        let (to_creator, fee) = split(amount, bps).unwrap();
        prop_assert_eq!(to_creator as u128 + fee as u128, amount as u128);
        prop_assert_eq!(fee as u128, amount as u128 * bps as u128 / BPS_DENOMINATOR as u128);
        prop_assert!(fee as u128 * BPS_DENOMINATOR as u128 <= amount as u128 * MAX_FEE_BPS as u128);
        prop_assert!(to_creator >= amount - amount / 20);
    }

    #[test]
    fn a_fee_above_the_cap_is_always_refused(amount in any::<u64>(), bps in (MAX_FEE_BPS + 1)..=u16::MAX) {
        prop_assert_eq!(split(amount, bps), None);
    }
}

proptest! {
    // Each case runs a whole LiteSVM, so fewer cases.
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn a_sol_tip_pays_exactly_amount_minus_fee(amount in 1u64..=50 * SOL, bps in 0u16..=MAX_FEE_BPS) {
        let mut env = ready_with_fee(bps);
        let (tipper, creator) = (env.user(), env.user());
        env.svm.airdrop(&tipper.pubkey(), 60 * SOL).unwrap();
        let treasury = env.treasury.pubkey();
        let (c0, t0, p0) = (env.lamports(&creator.pubkey()), env.lamports(&treasury), env.lamports(&tipper.pubkey()));
        env.send(&[tip_sol_ix(&tipper.pubkey(), &creator.pubkey(), &treasury, amount, reference(1))], &[&tipper]).unwrap();
        let (to_creator, fee) = split(amount, bps).unwrap();
        prop_assert_eq!(env.lamports(&creator.pubkey()) - c0, to_creator);
        prop_assert_eq!(env.lamports(&treasury) - t0, fee);
        prop_assert_eq!(p0 - env.lamports(&tipper.pubkey()), amount + 5_000);
    }

    #[test]
    fn a_token_purchase_pays_exactly_price_minus_fee_and_only_with_full_payment(
        price in 1u64..=1_000_000 * USDC,
        bps in 0u16..=MAX_FEE_BPS,
        short in any::<bool>(),
    ) {
        let mut env = ready_with_fee(bps);
        let (creator, buyer) = (env.user(), env.user());
        let treasury = env.treasury.pubkey();
        let post = env.register(&creator, [5; 32], price, Currency::Token);
        let balance = if short { price - 1 } else { price };
        let t = TokenAccounts {
            payer_token: env.token_account(&buyer.pubkey(), balance),
            creator: creator.pubkey(),
            creator_token: env.token_account(&creator.pubkey(), 0),
            mint: env.mint,
            treasury_token: Some(env.token_account(&treasury, 0)),
        };
        let out = env.send(&[buy_token_ix(&buyer.pubkey(), &post, &t, price)], &[&buyer]);
        let access = env.access(&access_pda(&post, &buyer.pubkey()));
        if short {
            prop_assert!(out.is_err());
            prop_assert!(access.is_none());
            prop_assert_eq!(env.tokens(&t.payer_token), price - 1);
            prop_assert_eq!(env.tokens(&t.creator_token), 0);
        } else {
            prop_assert!(out.is_ok());
            let (to_creator, fee) = split(price, bps).unwrap();
            prop_assert_eq!(env.tokens(&t.creator_token), to_creator);
            prop_assert_eq!(env.tokens(&t.treasury_token.unwrap()), fee);
            prop_assert_eq!(env.tokens(&t.payer_token), 0);
            prop_assert_eq!(access.unwrap().price, price);
        }
    }
}
