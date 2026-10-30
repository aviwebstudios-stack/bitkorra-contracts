//! Shared test setup: the built program in LiteSVM, with a known upgrade
//! authority written into its ProgramData account (LiteSVM deploys with
//! none), so `initialize`'s authority check runs for real.
#![allow(dead_code)]

use anchor_lang::{
    prelude::Pubkey,
    solana_program::{instruction::Instruction, system_program},
    AccountDeserialize, InstructionData, ToAccountMetas,
};
use beryl_names::state::{
    Config, NameRecord, PrimaryName, Records, CONFIG_SEED, NAME_SEED, PRIMARY_SEED,
};
use litesvm::LiteSVM;
use solana_keypair::Keypair;
use solana_message::{Message, VersionedMessage};
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;

pub const BPF_LOADER_UPGRADEABLE: Pubkey = anchor_lang::solana_program::bpf_loader_upgradeable::ID;

pub struct Env {
    pub svm: LiteSVM,
    pub upgrade_authority: Keypair,
    pub authority: Keypair,
}

pub fn program_id() -> Pubkey {
    beryl_names::id()
}

pub fn config_pda() -> Pubkey {
    Pubkey::find_program_address(&[CONFIG_SEED], &program_id()).0
}

pub fn name_pda(name: &str) -> Pubkey {
    Pubkey::find_program_address(&[NAME_SEED, name.as_bytes()], &program_id()).0
}

pub fn primary_pda(wallet: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[PRIMARY_SEED, wallet.as_ref()], &program_id()).0
}

pub fn programdata_pda() -> Pubkey {
    Pubkey::find_program_address(&[program_id().as_ref()], &BPF_LOADER_UPGRADEABLE).0
}

/// The program loaded, with `upgrade_authority` as its upgrade authority. Not initialized.
pub fn fresh() -> Env {
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(
        env!("CARGO_TARGET_TMPDIR"),
        "/../deploy/beryl_names.so"
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
    for k in [&upgrade_authority, &authority] {
        svm.airdrop(&k.pubkey(), 100_000_000_000).unwrap();
    }
    Env {
        svm,
        upgrade_authority,
        authority,
    }
}

/// Loaded and initialized: `authority` may register reserved names.
pub fn ready() -> Env {
    let mut env = fresh();
    let ua = env.upgrade_authority.insecure_clone();
    let authority = env.authority.pubkey();
    env.send(&[initialize_ix(&ua.pubkey(), authority)], &[&ua])
        .expect("initialize");
    env
}

impl Env {
    pub fn user(&mut self) -> Keypair {
        let k = Keypair::new();
        self.svm.airdrop(&k.pubkey(), 10_000_000_000).unwrap();
        k
    }

    /// Sends the instructions signed by `signers` (the first pays). Err holds the program logs.
    pub fn send(&mut self, ixs: &[Instruction], signers: &[&Keypair]) -> Result<(), String> {
        let payer = signers[0].pubkey();
        let msg = Message::new_with_blockhash(ixs, Some(&payer), &self.svm.latest_blockhash());
        let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), signers)
            .map_err(|e| e.to_string())?;
        let out = self.svm.send_transaction(tx);
        self.svm.expire_blockhash();
        match out {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("{:?}\n{}", e.err, e.meta.logs.join("\n"))),
        }
    }

    pub fn record(&self, name: &str) -> Option<NameRecord> {
        let a = self.svm.get_account(&name_pda(name))?;
        if a.data.is_empty() {
            return None;
        }
        NameRecord::try_deserialize(&mut a.data.as_slice()).ok()
    }

    pub fn primary(&self, wallet: &Pubkey) -> Option<PrimaryName> {
        let a = self.svm.get_account(&primary_pda(wallet))?;
        if a.data.is_empty() {
            return None;
        }
        PrimaryName::try_deserialize(&mut a.data.as_slice()).ok()
    }

    pub fn config(&self) -> Option<Config> {
        let a = self.svm.get_account(&config_pda())?;
        Config::try_deserialize(&mut a.data.as_slice()).ok()
    }

    pub fn lamports(&self, k: &Pubkey) -> u64 {
        self.svm.get_account(k).map(|a| a.lamports).unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// Instructions, built the way any client builds them.

pub fn initialize_ix(payer: &Pubkey, authority: Pubkey) -> Instruction {
    initialize_ix_with(payer, authority, programdata_pda())
}

pub fn initialize_ix_with(payer: &Pubkey, authority: Pubkey, program_data: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_names::instruction::Initialize { authority }.data(),
        beryl_names::accounts::Initialize {
            config: config_pda(),
            payer: *payer,
            program: program_id(),
            program_data,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

pub fn set_authority_ix(authority: &Pubkey, new_authority: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_names::instruction::SetAuthority { new_authority }.data(),
        beryl_names::accounts::SetAuthority {
            config: config_pda(),
            authority: *authority,
        }
        .to_account_metas(None),
    )
}

pub fn register_ix(owner: &Pubkey, name: &str) -> Instruction {
    register_ix_at(owner, name, name_pda(name))
}

/// Register with any account in the name's place (for the wrong-PDA tests).
pub fn register_ix_at(owner: &Pubkey, name: &str, record: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_names::instruction::Register {
            name: name.to_string(),
        }
        .data(),
        beryl_names::accounts::Register {
            name_record: record,
            config: config_pda(),
            owner: *owner,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

pub fn transfer_ix(owner: &Pubkey, name: &str, new_owner: Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_names::instruction::TransferName { new_owner }.data(),
        beryl_names::accounts::OwnerOnly {
            name_record: name_pda(name),
            owner: *owner,
        }
        .to_account_metas(None),
    )
}

pub fn update_ix(owner: &Pubkey, name: &str, records: Records) -> Instruction {
    update_ix_at(owner, name_pda(name), records)
}

pub fn update_ix_at(owner: &Pubkey, record: Pubkey, records: Records) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_names::instruction::UpdateRecords { records }.data(),
        beryl_names::accounts::OwnerOnly {
            name_record: record,
            owner: *owner,
        }
        .to_account_metas(None),
    )
}

pub fn set_primary_ix(owner: &Pubkey, name: &str) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_names::instruction::SetPrimary {}.data(),
        beryl_names::accounts::SetPrimary {
            name_record: name_pda(name),
            primary: primary_pda(owner),
            owner: *owner,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

pub fn clear_primary_ix(owner: &Pubkey) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_names::instruction::ClearPrimary {}.data(),
        beryl_names::accounts::ClearPrimary {
            primary: primary_pda(owner),
            owner: *owner,
        }
        .to_account_metas(None),
    )
}

pub fn release_ix(owner: &Pubkey, name: &str, with_primary: bool) -> Instruction {
    Instruction::new_with_bytes(
        program_id(),
        &beryl_names::instruction::Release {}.data(),
        beryl_names::accounts::Release {
            name_record: name_pda(name),
            primary: if with_primary {
                Some(primary_pda(owner))
            } else {
                None
            },
            owner: *owner,
        }
        .to_account_metas(None),
    )
}

/// The program's error, by its name in errors.rs, appears in the logs.
pub fn assert_error(result: Result<(), String>, error: &str) {
    match result {
        Ok(()) => panic!("expected {error}, but it succeeded"),
        Err(logs) => assert!(logs.contains(error), "expected {error}, got:\n{logs}"),
    }
}
