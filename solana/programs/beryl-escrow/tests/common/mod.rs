//! Shared test setup: Beryl Escrow in LiteSVM with a known upgrade authority
//! (so `initialize`'s check runs for real), a 6-decimal test token standing
//! in for devnet USDC, the clock under the test's control, and every
//! instruction built the way any client builds it.
#![allow(dead_code)]

use anchor_lang::{
    prelude::{Clock, Pubkey},
    solana_program::{instruction::Instruction, system_program},
    AccountDeserialize, AnchorDeserialize, Discriminator, InstructionData, ToAccountMetas,
};
use beryl_escrow::state::{Config, Escrow, CONFIG_SEED, ESCROW_SEED, VAULT_SEED};
use litesvm::LiteSVM;
use litesvm_token::{
    get_spl_account, spl_token::state::Account as SplAccount, CreateAssociatedTokenAccount,
    CreateMint, MintTo,
};
use solana_keypair::Keypair;
use solana_message::{Message, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;

pub const BPF_LOADER_UPGRADEABLE: Pubkey = anchor_lang::solana_program::bpf_loader_upgradeable::ID;
pub const TOKEN_PROGRAM: Pubkey = anchor_spl::token::ID;
pub const USDC: u64 = 1_000_000;
pub const SOL: u64 = 1_000_000_000;
pub const START: i64 = 1_800_000_000;
pub const WINDOW: i64 = 30 * 60;

pub struct Env {
    pub svm: LiteSVM,
    pub upgrade_authority: Keypair,
    pub owner: Keypair,
    pub arbiter: Keypair,
    pub treasury: Keypair,
    pub minter: Keypair,
    pub mint: Pubkey,
}

pub fn program_id() -> Pubkey {
    beryl_escrow::id()
}
pub fn config_pda() -> Pubkey {
    Pubkey::find_program_address(&[CONFIG_SEED], &program_id()).0
}
pub fn escrow_pda(seller: &Pubkey, order_id: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[ESCROW_SEED, seller.as_ref(), order_id], &program_id()).0
}
pub fn vault_pda(escrow: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[VAULT_SEED, escrow.as_ref()], &program_id()).0
}
pub fn programdata_pda() -> Pubkey {
    Pubkey::find_program_address(&[program_id().as_ref()], &BPF_LOADER_UPGRADEABLE).0
}
pub fn order(n: u8) -> [u8; 32] {
    [n; 32]
}

pub fn fresh() -> Env {
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/beryl_escrow.so"
    ));
    svm.add_program(program_id(), bytes).unwrap();
    let upgrade_authority = Keypair::new();
    let pd = programdata_pda();
    let mut account = svm.get_account(&pd).expect("programdata");
    account.data[12] = 1;
    account.data[13..45].copy_from_slice(upgrade_authority.pubkey().as_ref());
    svm.set_account(pd, account).unwrap();
    let (owner, arbiter, treasury, minter) = (
        Keypair::new(),
        Keypair::new(),
        Keypair::new(),
        Keypair::new(),
    );
    for k in [&upgrade_authority, &owner, &arbiter, &treasury, &minter] {
        svm.airdrop(&k.pubkey(), 100 * SOL).unwrap();
    }
    let mint = CreateMint::new(&mut svm, &minter)
        .decimals(6)
        .send()
        .expect("mint");
    let mut env = Env {
        svm,
        upgrade_authority,
        owner,
        arbiter,
        treasury,
        minter,
        mint,
    };
    env.set_time(START);
    env
}

/// Initialized with a 0 fee.
pub fn ready() -> Env {
    let mut env = fresh();
    let ua = env.upgrade_authority.insecure_clone();
    let (o, a, t, m) = (
        env.owner.pubkey(),
        env.arbiter.pubkey(),
        env.treasury.pubkey(),
        env.mint,
    );
    env.send(&[initialize_ix(&ua.pubkey(), o, a, t, m)], &[&ua])
        .expect("initialize");
    env
}

pub fn ready_with_fee(fee_bps: u16) -> Env {
    let mut env = ready();
    let owner = env.owner.insecure_clone();
    env.send(&[set_fee_ix(&owner.pubkey(), fee_bps)], &[&owner])
        .expect("set_fee");
    env
}

impl Env {
    pub fn set_time(&mut self, t: i64) {
        let mut c: Clock = self.svm.get_sysvar();
        c.unix_timestamp = t;
        self.svm.set_sysvar(&c);
    }
    pub fn now(&self) -> i64 {
        self.svm.get_sysvar::<Clock>().unix_timestamp
    }
    pub fn user(&mut self) -> Keypair {
        let k = Keypair::new();
        self.svm.airdrop(&k.pubkey(), 10 * SOL).unwrap();
        k
    }
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
    pub fn tokens(&self, account: &Pubkey) -> u64 {
        get_spl_account::<SplAccount>(&self.svm, account)
            .map(|a| a.amount)
            .unwrap_or(0)
    }
    pub fn lamports(&self, k: &Pubkey) -> u64 {
        self.svm.get_account(k).map(|a| a.lamports).unwrap_or(0)
    }
    pub fn exists(&self, k: &Pubkey) -> bool {
        self.svm
            .get_account(k)
            .map(|a| a.lamports > 0)
            .unwrap_or(false)
    }
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
        Config::try_deserialize(&mut self.svm.get_account(&config_pda()).unwrap().data.as_slice())
            .unwrap()
    }
    pub fn escrow(&self, key: &Pubkey) -> Option<Escrow> {
        let a = self.svm.get_account(key)?;
        if a.data.is_empty() {
            return None;
        }
        Escrow::try_deserialize(&mut a.data.as_slice()).ok()
    }
}

pub fn assert_error<T: std::fmt::Debug>(result: Result<T, String>, name: &str) {
    match result {
        Ok(v) => panic!("expected {name}, but it succeeded: {v:?}"),
        Err(e) => assert!(e.contains(name), "expected {name}, got:\n{e}"),
    }
}

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
        buf = (buf << 6) | ABC.iter().position(|&x| x == c)? as u32;
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
    owner: Pubkey,
    arbiter: Pubkey,
    treasury: Pubkey,
    mint: Pubkey,
) -> Instruction {
    initialize_ix_with(payer, owner, arbiter, treasury, mint, programdata_pda())
}

pub fn initialize_ix_with(
    payer: &Pubkey,
    owner: Pubkey,
    arbiter: Pubkey,
    treasury: Pubkey,
    mint: Pubkey,
    program_data: Pubkey,
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::Initialize {
            owner,
            arbiter,
            treasury,
        }
        .data(),
        beryl_escrow::accounts::Initialize {
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

fn owner_only(owner: &Pubkey) -> Vec<anchor_lang::solana_program::instruction::AccountMeta> {
    beryl_escrow::accounts::OwnerOnly {
        config: config_pda(),
        owner: *owner,
    }
    .to_account_metas(None)
}
pub fn set_fee_ix(owner: &Pubkey, fee_bps: u16) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::SetFee { fee_bps }.data(),
        owner_only(owner),
    )
}
pub fn set_treasury_ix(owner: &Pubkey, treasury: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::SetTreasury { treasury }.data(),
        owner_only(owner),
    )
}
pub fn propose_owner_ix(owner: &Pubkey, new_owner: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::ProposeOwner { new_owner }.data(),
        owner_only(owner),
    )
}
pub fn propose_arbiter_ix(owner: &Pubkey, new_arbiter: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::ProposeArbiter { new_arbiter }.data(),
        owner_only(owner),
    )
}
fn accept(new_key: &Pubkey) -> Vec<anchor_lang::solana_program::instruction::AccountMeta> {
    beryl_escrow::accounts::Accept {
        config: config_pda(),
        new_key: *new_key,
    }
    .to_account_metas(None)
}
pub fn accept_owner_ix(new_key: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::AcceptOwner {}.data(),
        accept(new_key),
    )
}
pub fn accept_arbiter_ix(new_key: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::AcceptArbiter {}.data(),
        accept(new_key),
    )
}

pub fn fund_sol_ix(
    seller: &Pubkey,
    order_id: [u8; 32],
    buyer: Pubkey,
    amount: u64,
    deadline: i64,
) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::FundSol {
            order_id,
            buyer,
            amount,
            deadline,
        }
        .data(),
        beryl_escrow::accounts::FundSol {
            seller: *seller,
            escrow: escrow_pda(seller, &order_id),
            config: config_pda(),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

pub fn fund_token_ix(
    seller: &Pubkey,
    seller_token: Pubkey,
    mint: Pubkey,
    order_id: [u8; 32],
    buyer: Pubkey,
    amount: u64,
    deadline: i64,
) -> Instruction {
    let escrow = escrow_pda(seller, &order_id);
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::FundToken {
            order_id,
            buyer,
            amount,
            deadline,
        }
        .data(),
        beryl_escrow::accounts::FundToken {
            seller: *seller,
            config: config_pda(),
            mint,
            seller_token,
            escrow,
            vault: vault_pda(&escrow),
            token_program: TOKEN_PROGRAM,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn party(
    signer: &Pubkey,
    escrow: &Pubkey,
) -> Vec<anchor_lang::solana_program::instruction::AccountMeta> {
    beryl_escrow::accounts::Party {
        signer: *signer,
        escrow: *escrow,
    }
    .to_account_metas(None)
}
pub fn mark_paid_ix(signer: &Pubkey, escrow: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::MarkPaid {}.data(),
        party(signer, escrow),
    )
}
pub fn open_dispute_ix(signer: &Pubkey, escrow: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::OpenDispute {}.data(),
        party(signer, escrow),
    )
}

/// The accounts every settlement takes. `token`: (buyer, seller, treasury) token accounts for a token escrow.
#[derive(Clone)]
pub struct SettleAccounts {
    pub escrow: Pubkey,
    pub seller: Pubkey,
    pub buyer: Pubkey,
    pub treasury: Option<Pubkey>,
    pub token: Option<(Pubkey, Pubkey, Option<Pubkey>, Pubkey)>,
}

fn settle_metas(
    signer: &Pubkey,
    s: &SettleAccounts,
) -> Vec<anchor_lang::solana_program::instruction::AccountMeta> {
    beryl_escrow::accounts::Settle {
        signer: *signer,
        escrow: s.escrow,
        seller: s.seller,
        buyer: s.buyer,
        config: config_pda(),
        treasury: s.treasury,
        vault: s.token.map(|_| vault_pda(&s.escrow)),
        buyer_token: s.token.map(|t| t.0),
        seller_token: s.token.map(|t| t.1),
        treasury_token: s.token.and_then(|t| t.2),
        mint: s.token.map(|t| t.3),
        token_program: s.token.map(|_| TOKEN_PROGRAM),
    }
    .to_account_metas(None)
}
pub fn release_ix(signer: &Pubkey, s: &SettleAccounts) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::Release {}.data(),
        settle_metas(signer, s),
    )
}
pub fn refund_ix(signer: &Pubkey, s: &SettleAccounts) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::Refund {}.data(),
        settle_metas(signer, s),
    )
}
pub fn cancel_ix(signer: &Pubkey, s: &SettleAccounts) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::Cancel {}.data(),
        settle_metas(signer, s),
    )
}
pub fn resolve_ix(signer: &Pubkey, s: &SettleAccounts, to_buyer: bool) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_escrow::instruction::Resolve { to_buyer }.data(),
        settle_metas(signer, s),
    )
}

/// A SOL escrow of `amount` from a fresh seller to a fresh buyer, deadline WINDOW from now.
pub fn sol_escrow(
    env: &mut Env,
    amount: u64,
) -> (
    solana_keypair::Keypair,
    solana_keypair::Keypair,
    SettleAccounts,
) {
    let (seller, buyer) = (env.user(), env.user());
    let id = order(7);
    let deadline = env.now() + WINDOW;
    env.send(
        &[fund_sol_ix(
            &seller.pubkey(),
            id,
            buyer.pubkey(),
            amount,
            deadline,
        )],
        &[&seller],
    )
    .expect("fund_sol");
    let s = SettleAccounts {
        escrow: escrow_pda(&seller.pubkey(), &id),
        seller: seller.pubkey(),
        buyer: buyer.pubkey(),
        treasury: Some(env.treasury.pubkey()),
        token: None,
    };
    (seller, buyer, s)
}

/// A token escrow of `amount` (the seller holds 100 USDC first).
pub fn token_escrow(
    env: &mut Env,
    amount: u64,
) -> (
    solana_keypair::Keypair,
    solana_keypair::Keypair,
    SettleAccounts,
) {
    let (seller, buyer) = (env.user(), env.user());
    let seller_token = env.token_account(&seller.pubkey(), 100 * USDC);
    let buyer_token = env.token_account(&buyer.pubkey(), 0);
    let treasury = env.treasury.pubkey();
    let treasury_token = env.token_account(&treasury, 0);
    let id = order(9);
    let deadline = env.now() + WINDOW;
    let mint = env.mint;
    env.send(
        &[fund_token_ix(
            &seller.pubkey(),
            seller_token,
            mint,
            id,
            buyer.pubkey(),
            amount,
            deadline,
        )],
        &[&seller],
    )
    .expect("fund_token");
    let s = SettleAccounts {
        escrow: escrow_pda(&seller.pubkey(), &id),
        seller: seller.pubkey(),
        buyer: buyer.pubkey(),
        treasury: Some(treasury),
        token: Some((buyer_token, seller_token, Some(treasury_token), mint)),
    };
    (seller, buyer, s)
}
