#![allow(dead_code)]
//! Shared LiteSVM setup helpers.

use litesvm::LiteSVM;
use solana_sdk::{
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    system_instruction,
    transaction::Transaction,
};
use token_2022_receive::constants::DEFAULT_RECEIPT_TTL_SLOTS;
use token_2022_receive::extension::receive_policy::SourceOwnerMode;
use token_2022_receive::extension::tlv::{account_len_with_receive_policy, unpack_account};
use token_2022_receive::guard::{derive_guard_state_address, derive_guard_token_address};
use token_2022_receive::instruction::{
    ensure_guard, initialize_account3, initialize_mint2, initialize_receive_policy, mint_to,
};
use token_2022_receive::state::{ACCOUNT_SIZE, MINT_SIZE};

fn program_so_path() -> std::path::PathBuf {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        std::env::var_os("CARGO_TARGET_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| manifest.join("../../target"))
            .join("deploy/token_2022_receive.so"),
        manifest.join("../../target/deploy/token_2022_receive.so"),
        manifest.join("target/deploy/token_2022_receive.so"),
    ];
    candidates
        .into_iter()
        .filter(|p| p.exists())
        .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .expect("missing token_2022_receive.so — run cargo build-sbf first")
}

pub fn token_amount(svm: &LiteSVM, key: &Pubkey) -> u64 {
    unpack_account(&svm.get_account(key).expect("account missing").data)
        .expect("token unpack")
        .amount
}

pub fn send(
    svm: &mut LiteSVM,
    payer: &Keypair,
    signers: &[&Keypair],
    ixs: Vec<solana_sdk::instruction::Instruction>,
) -> litesvm::types::TransactionResult {
    let mut all: Vec<&Keypair> = vec![payer];
    for s in signers {
        if s.pubkey() != payer.pubkey() {
            all.push(s);
        }
    }
    let tx = Transaction::new(
        &all,
        Message::new(&ixs, Some(&payer.pubkey())),
        svm.latest_blockhash(),
    );
    svm.send_transaction(tx)
}

pub struct Fixture {
    pub svm: LiteSVM,
    pub program_id: Pubkey,
    pub payer: Keypair,
    pub mint_authority: Keypair,
    pub source_owner: Keypair,
    pub dest_owner: Keypair,
    pub mint: Keypair,
    pub source: Keypair,
    pub dest: Keypair,
}

impl Fixture {
    pub fn boot(mint_amount: u64) -> Self {
        let program_id = token_2022_receive::id();
        let mut svm = LiteSVM::new();
        svm.add_program(
            program_id,
            &std::fs::read(program_so_path()).expect("read .so"),
        );

        let payer = Keypair::new();
        let mint_authority = Keypair::new();
        let source_owner = Keypair::new();
        let dest_owner = Keypair::new();
        let mint = Keypair::new();
        let source = Keypair::new();
        let dest = Keypair::new();
        svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();

        let rent_mint = svm.minimum_balance_for_rent_exemption(MINT_SIZE);
        let rent_acc = svm.minimum_balance_for_rent_exemption(ACCOUNT_SIZE);
        send(
            &mut svm,
            &payer,
            &[&mint],
            vec![system_instruction::create_account(
                &payer.pubkey(),
                &mint.pubkey(),
                rent_mint,
                MINT_SIZE as u64,
                &program_id,
            )],
        )
        .expect("create mint");
        send(
            &mut svm,
            &payer,
            &[],
            vec![initialize_mint2(
                &program_id,
                &mint.pubkey(),
                6,
                &mint_authority.pubkey(),
                None,
            )],
        )
        .expect("init mint");
        send(
            &mut svm,
            &payer,
            &[&source],
            vec![system_instruction::create_account(
                &payer.pubkey(),
                &source.pubkey(),
                rent_acc,
                ACCOUNT_SIZE as u64,
                &program_id,
            )],
        )
        .expect("create source");
        send(
            &mut svm,
            &payer,
            &[],
            vec![initialize_account3(
                &program_id,
                &source.pubkey(),
                &mint.pubkey(),
                &source_owner.pubkey(),
            )],
        )
        .expect("init source");
        send(
            &mut svm,
            &payer,
            &[&mint_authority],
            vec![mint_to(
                &program_id,
                &mint.pubkey(),
                &source.pubkey(),
                &mint_authority.pubkey(),
                mint_amount,
            )],
        )
        .expect("mint_to");

        Self {
            svm,
            program_id,
            payer,
            mint_authority,
            source_owner,
            dest_owner,
            mint,
            source,
            dest,
        }
    }

    pub fn with_plain_dest(mut self) -> Self {
        let rent_acc = self.svm.minimum_balance_for_rent_exemption(ACCOUNT_SIZE);
        send(
            &mut self.svm,
            &self.payer,
            &[&self.dest],
            vec![system_instruction::create_account(
                &self.payer.pubkey(),
                &self.dest.pubkey(),
                rent_acc,
                ACCOUNT_SIZE as u64,
                &self.program_id,
            )],
        )
        .expect("create dest");
        send(
            &mut self.svm,
            &self.payer,
            &[],
            vec![initialize_account3(
                &self.program_id,
                &self.dest.pubkey(),
                &self.mint.pubkey(),
                &self.dest_owner.pubkey(),
            )],
        )
        .expect("init dest");
        self
    }

    pub fn with_policy_dest(mut self, min_amount: u64) -> Self {
        let space = account_len_with_receive_policy();
        let rent_acc = self.svm.minimum_balance_for_rent_exemption(space);
        send(
            &mut self.svm,
            &self.payer,
            &[&self.dest],
            vec![system_instruction::create_account(
                &self.payer.pubkey(),
                &self.dest.pubkey(),
                rent_acc,
                space as u64,
                &self.program_id,
            )],
        )
        .expect("create policy dest");
        send(
            &mut self.svm,
            &self.payer,
            &[],
            vec![initialize_account3(
                &self.program_id,
                &self.dest.pubkey(),
                &self.mint.pubkey(),
                &self.dest_owner.pubkey(),
            )],
        )
        .expect("init policy dest");
        send(
            &mut self.svm,
            &self.payer,
            &[&self.dest_owner],
            vec![initialize_receive_policy(
                &self.program_id,
                &self.dest.pubkey(),
                &self.dest_owner.pubkey(),
                min_amount,
                SourceOwnerMode::AllowAll as u8,
                0,
                Pubkey::default(),
                1_000_000,
                DEFAULT_RECEIPT_TTL_SLOTS,
                vec![],
            )],
        )
        .expect("init receive policy");
        let (guard_token, _) = derive_guard_token_address(
            &self.dest_owner.pubkey(),
            &self.mint.pubkey(),
            &self.program_id,
        );
        let (guard_state, _) = derive_guard_state_address(
            &self.dest_owner.pubkey(),
            &self.mint.pubkey(),
            &self.program_id,
        );
        send(
            &mut self.svm,
            &self.payer,
            &[],
            vec![ensure_guard(
                &self.program_id,
                &self.payer.pubkey(),
                &self.dest_owner.pubkey(),
                &self.mint.pubkey(),
                &guard_token,
                &guard_state,
            )],
        )
        .expect("ensure_guard");
        self
    }

    /// Create an empty same-mint token account owned by `owner`.
    pub fn create_token_account(&mut self, owner: &Pubkey) -> Keypair {
        let account = Keypair::new();
        let rent_acc = self.svm.minimum_balance_for_rent_exemption(ACCOUNT_SIZE);
        send(
            &mut self.svm,
            &self.payer,
            &[&account],
            vec![system_instruction::create_account(
                &self.payer.pubkey(),
                &account.pubkey(),
                rent_acc,
                ACCOUNT_SIZE as u64,
                &self.program_id,
            )],
        )
        .expect("create token account");
        send(
            &mut self.svm,
            &self.payer,
            &[],
            vec![initialize_account3(
                &self.program_id,
                &account.pubkey(),
                &self.mint.pubkey(),
                owner,
            )],
        )
        .expect("init token account");
        account
    }
}
