//! Paid posts: register, change the price, close and reopen sales, buy in
//! SOL and in the token, the access account, and every refusal.

mod common;

use anchor_lang::prelude::Pubkey;
use beryl_tips::{
    state::Currency, PostPurchased, PostRegistered, PriceChanged, SalesStatusChanged,
};
use common::*;
use solana_signer::Signer;

const ID: [u8; 32] = [0xab; 32];

#[test]
fn a_creator_registers_a_post_once() {
    let mut env = ready();
    let creator = env.user();
    let logs = env
        .send(
            &[register_post_ix(
                &creator.pubkey(),
                ID,
                SOL / 4,
                Currency::Sol,
            )],
            &[&creator],
        )
        .unwrap();
    let key = post_pda(&creator.pubkey(), &ID);
    let p = env.post(&key).unwrap();
    assert_eq!(
        (p.creator, p.post_id, p.currency, p.mint),
        (creator.pubkey(), ID, Currency::Sol, Pubkey::default())
    );
    assert_eq!((p.price, p.open, p.sales), (SOL / 4, true, 0));
    let e: PostRegistered = event(&logs).unwrap();
    assert_eq!((e.post, e.price), (key, SOL / 4));
    // The same id again: the account already exists.
    assert!(env
        .send(
            &[register_post_ix(&creator.pubkey(), ID, SOL, Currency::Sol)],
            &[&creator]
        )
        .is_err());
    // Another creator may use the same id: their post lives at their own address.
    let other = env.user();
    let other_key = env.register(&other, ID, USDC, Currency::Token);
    assert_ne!(other_key, key);
    let p = env.post(&other_key).unwrap();
    assert_eq!((p.currency, p.mint), (Currency::Token, env.mint));
    assert_error(
        env.send(
            &[register_post_ix(
                &creator.pubkey(),
                [1; 32],
                0,
                Currency::Sol,
            )],
            &[&creator],
        ),
        "ZeroPrice",
    );
}

#[test]
fn only_the_creator_changes_the_price_or_closes_sales() {
    let mut env = ready();
    let (creator, stranger) = (env.user(), env.user());
    let post = env.register(&creator, ID, SOL, Currency::Sol);
    assert!(env
        .send(
            &[update_price_ix(&post, &stranger.pubkey(), 1)],
            &[&stranger]
        )
        .is_err());
    assert!(env
        .send(
            &[set_sales_open_ix(&post, &stranger.pubkey(), false)],
            &[&stranger]
        )
        .is_err());
    assert_error(
        env.send(&[update_price_ix(&post, &creator.pubkey(), 0)], &[&creator]),
        "ZeroPrice",
    );
    let logs = env
        .send(
            &[update_price_ix(&post, &creator.pubkey(), 2 * SOL)],
            &[&creator],
        )
        .unwrap();
    let e: PriceChanged = event(&logs).unwrap();
    assert_eq!((e.old_price, e.new_price), (SOL, 2 * SOL));
    let logs = env
        .send(
            &[set_sales_open_ix(&post, &creator.pubkey(), false)],
            &[&creator],
        )
        .unwrap();
    assert!(!event::<SalesStatusChanged>(&logs).unwrap().open);
    assert!(!env.post(&post).unwrap().open);
}

#[test]
fn buying_in_sol_pays_the_creator_and_proves_access() {
    let mut env = ready_with_fee(300);
    let (creator, buyer) = (env.user(), env.user());
    let treasury = env.treasury.pubkey();
    let post = env.register(&creator, ID, SOL / 2, Currency::Sol);
    let (c0, t0) = (env.lamports(&creator.pubkey()), env.lamports(&treasury));
    let logs = env
        .send(
            &[buy_sol_ix(
                &buyer.pubkey(),
                &post,
                &creator.pubkey(),
                &treasury,
                SOL / 2,
            )],
            &[&buyer],
        )
        .unwrap();
    let fee = SOL / 2 * 300 / 10_000;
    assert_eq!(env.lamports(&creator.pubkey()) - c0, SOL / 2 - fee);
    assert_eq!(env.lamports(&treasury) - t0, fee);
    let access_key = access_pda(&post, &buyer.pubkey());
    let a = env.access(&access_key).unwrap();
    assert_eq!(
        (a.post, a.buyer, a.price, a.mint),
        (post, buyer.pubkey(), SOL / 2, Pubkey::default())
    );
    assert_eq!(env.post(&post).unwrap().sales, 1);
    let e: PostPurchased = event(&logs).unwrap();
    assert_eq!(
        (e.post, e.buyer, e.access, e.price, e.fee, e.mint),
        (post, buyer.pubkey(), access_key, SOL / 2, fee, None)
    );
    // Nobody pays twice: the access account exists.
    assert!(env
        .send(
            &[buy_sol_ix(
                &buyer.pubkey(),
                &post,
                &creator.pubkey(),
                &treasury,
                SOL / 2
            )],
            &[&buyer]
        )
        .is_err());
    assert_eq!(env.post(&post).unwrap().sales, 1);
}

#[test]
fn closed_sales_refuse_new_buyers_and_existing_buyers_keep_access() {
    let mut env = ready();
    let (creator, early, late) = (env.user(), env.user(), env.user());
    let treasury = env.treasury.pubkey();
    let post = env.register(&creator, ID, SOL / 10, Currency::Sol);
    env.send(
        &[buy_sol_ix(
            &early.pubkey(),
            &post,
            &creator.pubkey(),
            &treasury,
            SOL / 10,
        )],
        &[&early],
    )
    .unwrap();
    env.send(
        &[set_sales_open_ix(&post, &creator.pubkey(), false)],
        &[&creator],
    )
    .unwrap();
    assert_error(
        env.send(
            &[buy_sol_ix(
                &late.pubkey(),
                &post,
                &creator.pubkey(),
                &treasury,
                SOL,
            )],
            &[&late],
        ),
        "SalesClosed",
    );
    assert!(env.access(&access_pda(&post, &early.pubkey())).is_some());
    // A price change doesn't touch access either.
    env.send(
        &[update_price_ix(&post, &creator.pubkey(), SOL)],
        &[&creator],
    )
    .unwrap();
    let a = env.access(&access_pda(&post, &early.pubkey())).unwrap();
    assert_eq!(a.price, SOL / 10);
    // Reopened: the late buyer can buy at the new price.
    env.send(
        &[set_sales_open_ix(&post, &creator.pubkey(), true)],
        &[&creator],
    )
    .unwrap();
    env.send(
        &[buy_sol_ix(
            &late.pubkey(),
            &post,
            &creator.pubkey(),
            &treasury,
            SOL,
        )],
        &[&late],
    )
    .unwrap();
    assert_eq!(env.post(&post).unwrap().sales, 2);
}

#[test]
fn a_price_raised_after_the_buyer_looked_is_refused() {
    let mut env = ready();
    let (creator, buyer) = (env.user(), env.user());
    let treasury = env.treasury.pubkey();
    let post = env.register(&creator, ID, SOL / 10, Currency::Sol);
    env.send(
        &[update_price_ix(&post, &creator.pubkey(), SOL)],
        &[&creator],
    )
    .unwrap();
    let b0 = env.lamports(&buyer.pubkey());
    assert_error(
        env.send(
            &[buy_sol_ix(
                &buyer.pubkey(),
                &post,
                &creator.pubkey(),
                &treasury,
                SOL / 10,
            )],
            &[&buyer],
        ),
        "PriceAboveMax",
    );
    assert!(env.access(&access_pda(&post, &buyer.pubkey())).is_none());
    assert!(
        b0 - env.lamports(&buyer.pubkey()) <= 5_000,
        "only the network fee"
    );
    // A lower price is fine with the same maximum: the buyer pays the actual price.
    env.send(
        &[update_price_ix(&post, &creator.pubkey(), SOL / 20)],
        &[&creator],
    )
    .unwrap();
    let c0 = env.lamports(&creator.pubkey());
    env.send(
        &[buy_sol_ix(
            &buyer.pubkey(),
            &post,
            &creator.pubkey(),
            &treasury,
            SOL / 10,
        )],
        &[&buyer],
    )
    .unwrap();
    assert_eq!(env.lamports(&creator.pubkey()) - c0, SOL / 20);
}

#[test]
fn sol_purchases_refuse_self_wrong_creator_wrong_treasury_wrong_access_and_currency() {
    let mut env = ready();
    let (creator, buyer, other) = (env.user(), env.user(), env.user());
    let treasury = env.treasury.pubkey();
    let post = env.register(&creator, ID, SOL / 10, Currency::Sol);
    assert_error(
        env.send(
            &[buy_sol_ix(
                &creator.pubkey(),
                &post,
                &creator.pubkey(),
                &treasury,
                SOL,
            )],
            &[&creator],
        ),
        "SelfPurchase",
    );
    // The money must go to the post's creator.
    assert_error(
        env.send(
            &[buy_sol_ix(
                &buyer.pubkey(),
                &post,
                &other.pubkey(),
                &treasury,
                SOL,
            )],
            &[&buyer],
        ),
        "WrongCreator",
    );
    assert_error(
        env.send(
            &[buy_sol_ix(
                &buyer.pubkey(),
                &post,
                &creator.pubkey(),
                &other.pubkey(),
                SOL,
            )],
            &[&buyer],
        ),
        "WrongTreasury",
    );
    // An access account at someone else's address (seeds of another buyer): refused.
    let wrong = access_pda(&post, &other.pubkey());
    assert!(env
        .send(
            &[buy_sol_ix_with(
                &buyer.pubkey(),
                &post,
                &creator.pubkey(),
                &treasury,
                SOL,
                wrong
            )],
            &[&buyer]
        )
        .is_err());
    // A token-priced post can't be bought with SOL.
    let token_post = env.register(&creator, [2; 32], USDC, Currency::Token);
    assert_error(
        env.send(
            &[buy_sol_ix(
                &buyer.pubkey(),
                &token_post,
                &creator.pubkey(),
                &treasury,
                u64::MAX,
            )],
            &[&buyer],
        ),
        "WrongCurrency",
    );
    // A fake "post" account (the config) is refused by its discriminator and seeds.
    assert!(env
        .send(
            &[buy_sol_ix(
                &buyer.pubkey(),
                &config_pda(),
                &creator.pubkey(),
                &treasury,
                SOL
            )],
            &[&buyer]
        )
        .is_err());
    assert_eq!(env.post(&post).unwrap().sales, 0);
}

#[test]
fn buying_in_the_token_pays_the_creator_and_the_fee() {
    let mut env = ready_with_fee(100);
    let (creator, buyer) = (env.user(), env.user());
    let treasury = env.treasury.pubkey();
    let post = env.register(&creator, ID, 3 * USDC, Currency::Token);
    let payer_token = env.token_account(&buyer.pubkey(), 5 * USDC);
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
            &[buy_token_ix(&buyer.pubkey(), &post, &t, 3 * USDC)],
            &[&buyer],
        ),
        "TreasuryAccountMissing",
    );
    t.treasury_token = Some(treasury_token);
    assert_error(
        env.send(
            &[buy_token_ix(&buyer.pubkey(), &post, &t, 3 * USDC - 1)],
            &[&buyer],
        ),
        "PriceAboveMax",
    );
    let logs = env
        .send(
            &[buy_token_ix(&buyer.pubkey(), &post, &t, 3 * USDC)],
            &[&buyer],
        )
        .unwrap();
    assert_eq!(env.tokens(&creator_token), 3 * USDC - 30_000);
    assert_eq!(env.tokens(&treasury_token), 30_000);
    assert_eq!(env.tokens(&payer_token), 2 * USDC);
    let a = env.access(&access_pda(&post, &buyer.pubkey())).unwrap();
    assert_eq!((a.price, a.mint), (3 * USDC, env.mint));
    let e: PostPurchased = event(&logs).unwrap();
    assert_eq!((e.mint, e.fee), (Some(env.mint), 30_000));
    assert!(env
        .send(
            &[buy_token_ix(&buyer.pubkey(), &post, &t, 3 * USDC)],
            &[&buyer]
        )
        .is_err());
}

#[test]
fn token_purchases_check_currency_mint_owners_and_balance() {
    let mut env = ready();
    let (creator, buyer, other) = (env.user(), env.user(), env.user());
    let token_post = env.register(&creator, ID, USDC, Currency::Token);
    let sol_post = env.register(&creator, [3; 32], USDC, Currency::Sol);
    let payer_token = env.token_account(&buyer.pubkey(), 10 * USDC);
    let creator_token = env.token_account(&creator.pubkey(), 0);
    let others_token = env.token_account(&other.pubkey(), 0);
    let mint = env.mint;
    let base = || TokenAccounts {
        payer_token,
        creator: creator.pubkey(),
        creator_token,
        mint,
        treasury_token: None,
    };
    assert_error(
        env.send(
            &[buy_token_ix(&buyer.pubkey(), &sol_post, &base(), USDC)],
            &[&buyer],
        ),
        "WrongCurrency",
    );
    // Paying someone other than the creator: refused.
    let mut t = base();
    t.creator_token = others_token;
    assert!(env
        .send(
            &[buy_token_ix(&buyer.pubkey(), &token_post, &t, USDC)],
            &[&buyer]
        )
        .is_err());
    let mut t = base();
    t.creator = other.pubkey();
    t.creator_token = others_token;
    assert!(env
        .send(
            &[buy_token_ix(&buyer.pubkey(), &token_post, &t, USDC)],
            &[&buyer]
        )
        .is_err());
    // Another mint: refused.
    let (other_mint, other_ata) = env.other_mint_account(&buyer.pubkey(), 10 * USDC);
    let mut t = base();
    t.mint = other_mint;
    t.payer_token = other_ata;
    assert_error(
        env.send(
            &[buy_token_ix(&buyer.pubkey(), &token_post, &t, USDC)],
            &[&buyer],
        ),
        "WrongMint",
    );
    // Not enough tokens: the whole purchase fails, no access.
    let poor = env.user();
    let poor_token = env.token_account(&poor.pubkey(), USDC - 1);
    let mut t = base();
    t.payer_token = poor_token;
    assert!(env
        .send(
            &[buy_token_ix(&poor.pubkey(), &token_post, &t, USDC)],
            &[&poor]
        )
        .is_err());
    assert!(env
        .access(&access_pda(&token_post, &poor.pubkey()))
        .is_none());
    assert_eq!(env.tokens(&creator_token), 0);
    assert_eq!(env.post(&token_post).unwrap().sales, 0);
}
