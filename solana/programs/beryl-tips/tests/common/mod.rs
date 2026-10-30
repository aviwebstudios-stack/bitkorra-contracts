//! Shared test setup: Beryl Tips in LiteSVM with a known upgrade authority
//! written into its ProgramData (LiteSVM deploys with none, so
//! `initialize`'s check runs for real), a 6-decimal test token standing in
//! for devnet USDC, and every instruction built the way any client builds it.
#![allow(dead_code)]

use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    AccountDeserialize, AnchorDeserialize, Discriminator, InstructionData, ToAccountMetas,
};
use beryl_tips::state::{Access, Config, Currency, Post, ACCESS_SEED, CONFIG_SEED, POST_SEED};
use litesvm::LiteSVM;
use litesvm_token::{
    get_spl_account, spl_token::state::Account as SplAccount, CreateAccount,
    CreateAssociatedTokenAccount, CreateMint, MintTo,
};
use solana_keypair::Keypair;
use solana_message::{Message, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;

pub const BPF_LOADER_UPGRADEABLE: Pubkey = anchor_lang::solana_program::bpf_loader_upgradeable::ID;
pub const TOKEN_PROGRAM: Pubkey = anchor_spl::token::ID;
/// 1 test USDC (6 decimals).
pub const USDC: u64 = 1_000_000;
pub const SOL: u64 = 1_000_000_000;

pub struct Env {
    pub svm: LiteSVM,
    pub upgrade_authority: Keypair,
    pub authority: Keypair,
    pub treasury: Keypair,
    /// Mint authority of the test token.
    pub minter: Keypair,
    pub mint: Pubkey,
}

pub fn program_id() -> Pubkey {
    beryl_tips::id()
}

pub fn config_pda() -> Pubkey {
    Pubkey::find_program_address(&[CONFIG_SEED], &program_id()).0
}

pub fn post_pda(creator: &Pubkey, post_id: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[POST_SEED, creator.as_ref(), post_id], &program_id()).0
}

pub fn access_pda(post: &Pubkey, buyer: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[ACCESS_SEED, post.as_ref(), buyer.as_ref()], &program_id()).0
}

pub fn programdata_pda() -> Pubkey {
    Pubkey::find_program_address(&[program_id().as_ref()], &BPF_LOADER_UPGRADEABLE).0
}

pub fn reference(n: u8) -> [u8; 32] {
    [n; 32]
}

/// The program loaded, with `upgrade_authority` as its upgrade authority, and the test mint. Not initialized.
pub fn fresh() -> Env {
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/beryl_tips.so"
    ));
    svm.add_program(program_id(), bytes).unwrap();
    let upgrade_authority = Keypair::new();
    // ProgramData's header: u32 tag, u64 slot, then Option<Pubkey> (1 + 32 bytes) at offset 12.
    let pd = programdata_pda();
    let mut account = svm.get_account(&pd).expect("programdata");
    account.data[12] = 1;
    account.data[13..45].copy_from_slice(upgrade_authority.pubkey().as_ref());
    svm.set_account(pd, account).unwrap();
    let authority = Keypair::new();
    let treasury = Keypair::new();
    let minter = Keypair::new();
    for k in [&upgrade_authority, &authority, &treasury, &minter] {
        svm.airdrop(&k.pubkey(), 100 * SOL).unwrap();
    }
    let mint = CreateMint::new(&mut svm, &minter)
        .decimals(6)
        .send()
        .expect("mint");
    Env {
        svm,
        upgrade_authority,
        authority,
        treasury,
        minter,
        mint,
    }
}

/// Loaded and initialized with a 0 fee.
pub fn ready() -> Env {
    let mut env = fresh();
    let ua = env.upgrade_authority.insecure_clone();
    let (authority, treasury, mint) = (env.authority.pubkey(), env.treasury.pubkey(), env.mint);
    env.send(
        &[initialize_ix(&ua.pubkey(), authority, treasury, mint)],
        &[&ua],
    )
    .expect("initialize");
    env
}

/// Initialized with `fee_bps`.
pub fn ready_with_fee(fee_bps: u16) -> Env {
    let mut env = ready();
    let auth = env.authority.insecure_clone();
    env.send(&[set_fee_ix(&auth.pubkey(), fee_bps)], &[&auth])
        .expect("set_fee");
    env
}

impl Env {
    pub fn user(&mut self) -> Keypair {
        let k = Keypair::new();
        self.svm.airdrop(&k.pubkey(), 10 * SOL).unwrap();
        k
    }

    /// A token account (the associated one) for `owner`, holding `amount`.
    pub fn token_account(&mut self, owner: &Pubkey, amount: u64) -> Pubkey {
        let minter = self.minter.insecure_clone();
        let mint = self.mint;
        let ata = CreateAssociatedTokenAccount::new(&mut self.svm, &minter, &mint)
            .owner(owner)
            .send()
            .expect("ata");
        if amount > 0 {
            MintTo::new(&mut self.svm, &minter, &mint, &ata, amount)
                .send()
                .expect("mint_to");
        }
        ata
    }

    /// A second (non-associated) token account of the accepted mint for `owner`.
    pub fn extra_token_account(&mut self, owner: &Pubkey) -> Pubkey {
        let minter = self.minter.insecure_clone();
        let mint = self.mint;
        CreateAccount::new(&mut self.svm, &minter, &mint)
            .owner(owner)
            .send()
            .expect("token account")
    }

    /// A token account of another mint (not the accepted one), for `owner`.
    pub fn other_mint_account(&mut self, owner: &Pubkey, amount: u64) -> (Pubkey, Pubkey) {
        let minter = self.minter.insecure_clone();
        let other = CreateMint::new(&mut self.svm, &minter)
            .decimals(6)
            .send()
            .unwrap();
        let ata = CreateAssociatedTokenAccount::new(&mut self.svm, &minter, &other)
            .owner(owner)
            .send()
            .unwrap();
        MintTo::new(&mut self.svm, &minter, &other, &ata, amount)
            .send()
            .unwrap();
        (other, ata)
    }

    pub fn tokens(&self, account: &Pubkey) -> u64 {
        get_spl_account::<SplAccount>(&self.svm, account)
            .map(|a| a.amount)
            .unwrap_or(0)
    }

    /// Sends the instructions signed by `signers` (the first pays). Ok holds the logs, Err the error and logs.
    pub fn send(
        &mut self,
        ixs: &[Instruction],
        signers: &[&Keypair],
    ) -> Result<Vec<String>, String> {
        let payer = signers[0].pubkey();
        let msg = Message::new_with_blockhash(ixs, Some(&payer), &self.svm.latest_blockhash());
        let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers)
            .map_err(|e| e.to_string())?;
        let out = self.svm.send_transaction(tx);
        self.svm.expire_blockhash();
        match out {
            Ok(meta) => Ok(meta.logs),
            Err(e) => Err(format!("{:?}\n{}", e.err, e.meta.logs.join("\n"))),
        }
    }

    pub fn config(&self) -> Config {
        let a = self.svm.get_account(&config_pda()).expect("config");
        Config::try_deserialize(&mut a.data.as_slice()).unwrap()
    }

    pub fn post(&self, key: &Pubkey) -> Option<Post> {
        let a = self.svm.get_account(key)?;
        if a.data.is_empty() {
            return None;
        }
        Post::try_deserialize(&mut a.data.as_slice()).ok()
    }

    pub fn access(&self, key: &Pubkey) -> Option<Access> {
        let a = self.svm.get_account(key)?;
        if a.data.is_empty() {
            return None;
        }
        Access::try_deserialize(&mut a.data.as_slice()).ok()
    }

    pub fn lamports(&self, k: &Pubkey) -> u64 {
        self.svm.get_account(k).map(|a| a.lamports).unwrap_or(0)
    }

    /// Registers a post for `creator` and returns its address.
    pub fn register(
        &mut self,
        creator: &Keypair,
        post_id: [u8; 32],
        price: u64,
        currency: Currency,
    ) -> Pubkey {
        self.send(
            &[register_post_ix(
                &creator.pubkey(),
                post_id,
                price,
                currency,
            )],
            &[creator],
        )
        .expect("register_post");
        post_pda(&creator.pubkey(), &post_id)
    }
}

pub fn assert_error<T: std::fmt::Debug>(result: Result<T, String>, name: &str) {
    match result {
        Ok(v) => panic!("expected {name}, but it succeeded: {v:?}"),
        Err(e) => assert!(e.contains(name), "expected {name}, got:\n{e}"),
    }
}

/// The first event of type `E` in the logs (Anchor's `emit!`: "Program data: " + base64 of discriminator and Borsh).
pub fn event<E: AnchorDeserialize + Discriminator>(logs: &[String]) -> Option<E> {
    logs.iter().find_map(|line| {
        let data = base64(line.strip_prefix("Program data: ")?)?;
        let rest = data.strip_prefix(E::DISCRIMINATOR)?;
        E::deserialize(&mut &rest[..]).ok()
    })
}

fn base64(s: &str) -> Option<Vec<u8>> {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.trim_end_matches('=').bytes() {
        let v = ABC.iter().position(|&x| x == c)? as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Instructions

pub fn initialize_ix(
    payer: &Pubkey,
    authority: Pubkey,
    treasury: Pubkey,
    mint: Pubkey,
) -> Instruction {
    initialize_ix_with(payer, authority, treasury, mint, programdata_pda())
}

pub fn initialize_ix_with(
    payer: &Pubkey,
    authority: Pubkey,
    treasury: Pubkey,
    mint: Pubkey,
    program_data: Pubkey,
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_tips::instruction::Initialize {
            authority,
            treasury,
        }
        .data(),
        beryl_tips::accounts::Initialize {
            config: config_pda(),
            payer: *payer,
            mint,
            program: program_id(),
            program_data,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn authority_only(
    authority: &Pubkey,
) -> Vec<anchor_lang::solana_program::instruction::AccountMeta> {
    beryl_tips::accounts::AuthorityOnly {
        config: config_pda(),
        authority: *authority,
    }
    .to_account_metas(None)
}

pub fn set_fee_ix(authority: &Pubkey, fee_bps: u16) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_tips::instruction::SetFee { fee_bps }.data(),
        authority_only(authority),
    )
}

pub fn set_treasury_ix(authority: &Pubkey, treasury: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_tips::instruction::SetTreasury { treasury }.data(),
        authority_only(authority),
    )
}

pub fn propose_authority_ix(authority: &Pubkey, new_authority: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_tips::instruction::ProposeAuthority { new_authority }.data(),
        authority_only(authority),
    )
}

pub fn accept_authority_ix(new_authority: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_tips::instruction::AcceptAuthority {}.data(),
        beryl_tips::accounts::AcceptAuthority {
            config: config_pda(),
            new_authority: *new_authority,
        }
        .to_account_metas(None),
    )
}

pub fn tip_sol_ix(
    tipper: &Pubkey,
    creator: &Pubkey,
    treasury: &Pubkey,
    amount: u64,
    reference: [u8; 32],
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_tips::instruction::TipSol { amount, reference }.data(),
        beryl_tips::accounts::TipSol {
            tipper: *tipper,
            creator: *creator,
            config: config_pda(),
            treasury: *treasury,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

pub struct TokenAccounts {
    pub payer_token: Pubkey,
    pub creator: Pubkey,
    pub creator_token: Pubkey,
    pub mint: Pubkey,
    pub treasury_token: Option<Pubkey>,
}

pub fn tip_token_ix(
    tipper: &Pubkey,
    t: &TokenAccounts,
    amount: u64,
    reference: [u8; 32],
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_tips::instruction::TipToken { amount, reference }.data(),
        beryl_tips::accounts::TipToken {
            tipper: *tipper,
            tipper_token: t.payer_token,
            creator: t.creator,
            creator_token: t.creator_token,
            config: config_pda(),
            mint: t.mint,
            treasury_token: t.treasury_token,
            token_program: TOKEN_PROGRAM,
        }
        .to_account_metas(None),
    )
}

pub fn register_post_ix(
    creator: &Pubkey,
    post_id: [u8; 32],
    price: u64,
    currency: Currency,
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_tips::instruction::RegisterPost {
            post_id,
            price,
            currency,
        }
        .data(),
        beryl_tips::accounts::RegisterPost {
            post: post_pda(creator, &post_id),
            creator: *creator,
            config: config_pda(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn creator_only(
    post: &Pubkey,
    creator: &Pubkey,
) -> Vec<anchor_lang::solana_program::instruction::AccountMeta> {
    beryl_tips::accounts::CreatorOnly {
        post: *post,
        creator: *creator,
    }
    .to_account_metas(None)
}

pub fn update_price_ix(post: &Pubkey, creator: &Pubkey, new_price: u64) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_tips::instruction::UpdatePrice { new_price }.data(),
        creator_only(post, creator),
    )
}

pub fn set_sales_open_ix(post: &Pubkey, creator: &Pubkey, open: bool) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_tips::instruction::SetSalesOpen { open }.data(),
        creator_only(post, creator),
    )
}

pub fn buy_sol_ix(
    buyer: &Pubkey,
    post: &Pubkey,
    creator: &Pubkey,
    treasury: &Pubkey,
    max_price: u64,
) -> Instruction {
    buy_sol_ix_with(
        buyer,
        post,
        creator,
        treasury,
        max_price,
        access_pda(post, buyer),
    )
}

pub fn buy_sol_ix_with(
    buyer: &Pubkey,
    post: &Pubkey,
    creator: &Pubkey,
    treasury: &Pubkey,
    max_price: u64,
    access: Pubkey,
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_tips::instruction::BuySol { max_price }.data(),
        beryl_tips::accounts::BuySol {
            buyer: *buyer,
            post: *post,
            creator: *creator,
            access,
            config: config_pda(),
            treasury: *treasury,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

pub fn buy_token_ix(
    buyer: &Pubkey,
    post: &Pubkey,
    t: &TokenAccounts,
    max_price: u64,
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_tips::instruction::BuyToken { max_price }.data(),
        beryl_tips::accounts::BuyToken {
            buyer: *buyer,
            buyer_token: t.payer_token,
            post: *post,
            creator: t.creator,
            creator_token: t.creator_token,
            access: access_pda(post, buyer),
            config: config_pda(),
            mint: t.mint,
            treasury_token: t.treasury_token,
            token_program: TOKEN_PROGRAM,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}
