//! proptest: the release split on its own, then real escrows with random
//! amounts, fees and endings. Money is only ever conserved between the
//! seller, the buyer and the treasury.

mod common;

use beryl_escrow::state::{release_split, BPS, MAX_FEE_BPS};
use common::*;
use proptest::prelude::*;
use solana_signer::Signer;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(5_000))]

    #[test]
    fn the_split_adds_up_and_never_passes_2_percent(amount in any::<u64>(), bps in 0u16..=MAX_FEE_BPS) {
        let (to_buyer, fee) = release_split(amount, bps).unwrap();
        prop_assert_eq!(to_buyer as u128 + fee as u128, amount as u128);
        prop_assert_eq!(fee as u128, amount as u128 * bps as u128 / BPS as u128);
        prop_assert!(fee <= amount / 50);
    }

    #[test]
    fn any_fee_above_the_cap_is_refused(amount in any::<u64>(), bps in (MAX_FEE_BPS + 1)..=u16::MAX) {
        prop_assert_eq!(release_split(amount, bps), None);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// 0 release, 1 refund after the deadline, 2 buyer cancel, 3 dispute to buyer, 4 dispute to seller.
    #[test]
    fn every_ending_conserves_money_between_the_three(amount in 1u64..=9 * SOL, bps in 0u16..=MAX_FEE_BPS, ending in 0u8..5) {
        let mut env = ready_with_fee(bps);
        let (seller, buyer, s) = sol_escrow(&mut env, amount);
        let arbiter = env.arbiter.insecure_clone();
        let treasury = env.treasury.pubkey();
        let rent = env.lamports(&s.escrow) - amount;
        let (b0, s0, t0) = (env.lamports(&buyer.pubkey()), env.lamports(&seller.pubkey()), env.lamports(&treasury));
        let mut seller_fees = 0u64;
        let mut buyer_fees = 0u64;
        match ending {
            0 => { env.send(&[release_ix(&seller.pubkey(), &s)], &[&seller]).unwrap(); seller_fees += 5_000; }
            1 => { let t = env.now(); env.set_time(t + WINDOW + 1); env.send(&[refund_ix(&seller.pubkey(), &s)], &[&seller]).unwrap(); seller_fees += 5_000; }
            2 => { env.send(&[cancel_ix(&buyer.pubkey(), &s)], &[&buyer]).unwrap(); buyer_fees += 5_000; }
            _ => {
                env.send(&[mark_paid_ix(&buyer.pubkey(), &s.escrow)], &[&buyer]).unwrap();
                env.send(&[open_dispute_ix(&buyer.pubkey(), &s.escrow)], &[&buyer]).unwrap();
                buyer_fees += 10_000;
                env.send(&[resolve_ix(&arbiter.pubkey(), &s, ending == 3)], &[&arbiter]).unwrap();
            }
        }
        let to_buyer = ending == 0 || ending == 3;
        let (net, fee) = if to_buyer { release_split(amount, bps).unwrap() } else { (0, 0) };
        prop_assert_eq!(env.lamports(&buyer.pubkey()) + buyer_fees - b0, net);
        prop_assert_eq!(env.lamports(&treasury) - t0, fee);
        prop_assert_eq!(env.lamports(&seller.pubkey()) + seller_fees - s0, rent + if to_buyer { 0 } else { amount });
        prop_assert!(!env.exists(&s.escrow));
        // Nothing can settle twice.
        prop_assert!(env.send(&[release_ix(&seller.pubkey(), &s)], &[&seller]).is_err());
    }

    #[test]
    fn token_escrows_pay_exactly_and_leave_nothing_behind(amount in 1u64..=100 * USDC, bps in 0u16..=MAX_FEE_BPS, to_buyer in any::<bool>()) {
        let mut env = ready_with_fee(bps);
        let (seller, buyer, s) = token_escrow(&mut env, amount);
        let (bt, st, tt, _) = s.token.unwrap();
        env.send(&[mark_paid_ix(&buyer.pubkey(), &s.escrow)], &[&buyer]).unwrap();
        if to_buyer {
            env.send(&[release_ix(&seller.pubkey(), &s)], &[&seller]).unwrap();
        } else {
            let arbiter = env.arbiter.insecure_clone();
            env.send(&[open_dispute_ix(&seller.pubkey(), &s.escrow)], &[&seller]).unwrap();
            env.send(&[resolve_ix(&arbiter.pubkey(), &s, false)], &[&arbiter]).unwrap();
        }
        let (net, fee) = if to_buyer { release_split(amount, bps).unwrap() } else { (0, 0) };
        prop_assert_eq!(env.tokens(&bt), net);
        prop_assert_eq!(env.tokens(&tt.unwrap()), fee);
        prop_assert_eq!(env.tokens(&st), 100 * USDC - amount + if to_buyer { 0 } else { amount });
        prop_assert!(!env.exists(&vault_pda(&s.escrow)));
    }
}
