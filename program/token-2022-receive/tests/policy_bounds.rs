//! Regression suite for receiver-controlled ReceivePolicy fields.
//!
//! The destination owner writes the policy, but the *sender* pays for it: in lamports (the
//! receipt bond is debited from bond_payer) and in time (TTL decides how long a rejected
//! transfer stays locked). Those fields are therefore attacker-controlled input from the
//! sender's point of view and must be validated and bounded at the boundary.
//!
//! ```bash
//! cargo build-sbf --manifest-path program/token-2022-receive/Cargo.toml
//! cargo test -p token-2022-receive --test policy_bounds
//! ```

#[path = "common/litesvm.rs"]
mod litesvm_helpers;

use litesvm_helpers::{send, Fixture};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use token_2022_receive::constants::{
    DEFAULT_RECEIPT_TTL_SLOTS, MAX_RECEIPT_BOND_LAMPORTS, MAX_RECEIPT_TTL_SLOTS,
};
use token_2022_receive::extension::receive_policy::SourceOwnerMode;
use token_2022_receive::extension::tlv::account_len_with_receive_policy;
use token_2022_receive::instruction::{initialize_account3, initialize_receive_policy};

/// Allocate + init a bare token account sized for a policy, owned by `owner`.
fn policy_ready_account(fx: &mut Fixture, owner: &Pubkey) -> Keypair {
    let space = account_len_with_receive_policy();
    let rent = fx.svm.minimum_balance_for_rent_exemption(space);
    let acct = Keypair::new();
    send(
        &mut fx.svm,
        &fx.payer,
        &[&acct],
        vec![solana_sdk::system_instruction::create_account(
            &fx.payer.pubkey(),
            &acct.pubkey(),
            rent,
            space as u64,
            &fx.program_id,
        )],
    )
    .expect("create");
    send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![initialize_account3(
            &fx.program_id,
            &acct.pubkey(),
            &fx.mint.pubkey(),
            owner,
        )],
    )
    .expect("init");
    fx.svm.expire_blockhash();
    acct
}

#[allow(clippy::too_many_arguments)]
fn init_policy(
    fx: &mut Fixture,
    account: &Pubkey,
    owner: &Keypair,
    min_amount: u64,
    source_owner_mode: u8,
    recovery_authority_mode: u8,
    bond: u64,
    ttl: u64,
) -> litesvm::types::TransactionResult {
    let r = send(
        &mut fx.svm,
        &fx.payer,
        &[owner],
        vec![initialize_receive_policy(
            &fx.program_id,
            account,
            &owner.pubkey(),
            min_amount,
            source_owner_mode,
            recovery_authority_mode,
            Pubkey::default(),
            bond,
            ttl,
            vec![],
        )],
    );
    fx.svm.expire_blockhash();
    r
}

#[test]
fn policy_is_write_once() {
    let mut fx = Fixture::boot(1_000);
    let owner = fx.dest_owner.insecure_clone();
    let acct = policy_ready_account(&mut fx, &owner.pubkey());

    init_policy(&mut fx, &acct.pubkey(), &owner, 0, 0, 0, 0, DEFAULT_RECEIPT_TTL_SLOTS)
        .expect("first policy write");

    // Rewriting in place would let the receiver flip min_amount / recovery authority / TTL
    // between a sender's quote and the sender's transaction.
    let err = init_policy(
        &mut fx,
        &acct.pubkey(),
        &owner,
        u64::MAX,
        0,
        1,
        0,
        DEFAULT_RECEIPT_TTL_SLOTS,
    )
    .expect_err("policy must not be rewritable in place");
    eprintln!("[policy re-init blocked] {err:?}");
}

#[test]
fn out_of_range_source_owner_mode_is_rejected() {
    let mut fx = Fixture::boot(1_000);
    let owner = fx.dest_owner.insecure_clone();
    let acct = policy_ready_account(&mut fx, &owner.pubkey());

    // Previously stored verbatim and decoded fail-open to AllowAll, silently disabling an
    // allowlist the receiver believed was in force.
    init_policy(&mut fx, &acct.pubkey(), &owner, 0, 7, 0, 0, DEFAULT_RECEIPT_TTL_SLOTS)
        .expect_err("unknown source_owner_mode must be rejected");

    init_policy(
        &mut fx,
        &acct.pubkey(),
        &owner,
        0,
        SourceOwnerMode::Allowlist as u8,
        0,
        0,
        DEFAULT_RECEIPT_TTL_SLOTS,
    )
    .expect("known mode still accepted");
}

#[test]
fn out_of_range_recovery_authority_mode_is_rejected() {
    let mut fx = Fixture::boot(1_000);
    let owner = fx.dest_owner.insecure_clone();
    let acct = policy_ready_account(&mut fx, &owner.pubkey());

    init_policy(&mut fx, &acct.pubkey(), &owner, 0, 0, 9, 0, DEFAULT_RECEIPT_TTL_SLOTS)
        .expect_err("unknown recovery_authority_mode must be rejected");
}

#[test]
fn receipt_bond_is_capped() {
    let mut fx = Fixture::boot(1_000);
    let owner = fx.dest_owner.insecure_clone();
    let acct = policy_ready_account(&mut fx, &owner.pubkey());

    // The bond is debited from the sender-side bond_payer, so an unbounded value is a
    // griefing lever against senders.
    init_policy(
        &mut fx,
        &acct.pubkey(),
        &owner,
        0,
        0,
        0,
        MAX_RECEIPT_BOND_LAMPORTS + 1,
        DEFAULT_RECEIPT_TTL_SLOTS,
    )
    .expect_err("bond above the cap must be rejected");

    init_policy(
        &mut fx,
        &acct.pubkey(),
        &owner,
        0,
        0,
        0,
        MAX_RECEIPT_BOND_LAMPORTS,
        DEFAULT_RECEIPT_TTL_SLOTS,
    )
    .expect("bond at the cap is accepted");
}

#[test]
fn receipt_ttl_is_capped() {
    let mut fx = Fixture::boot(1_000);
    let owner = fx.dest_owner.insecure_clone();
    let acct = policy_ready_account(&mut fx, &owner.pubkey());

    // An unbounded TTL under Receiver / ThirdParty recovery holds a rejected transfer
    // hostage indefinitely.
    init_policy(&mut fx, &acct.pubkey(), &owner, 0, 0, 0, 0, u64::MAX / 2)
        .expect_err("TTL above the cap must be rejected");

    init_policy(&mut fx, &acct.pubkey(), &owner, 0, 0, 0, 0, MAX_RECEIPT_TTL_SLOTS)
        .expect("TTL at the cap is accepted");
}

#[test]
fn policy_requires_a_program_owned_account() {
    let mut fx = Fixture::boot(1_000);
    let owner = fx.dest_owner.insecure_clone();
    let foreign = Keypair::new();
    fx.svm.airdrop(&foreign.pubkey(), 10_000_000).unwrap();
    fx.svm.expire_blockhash();

    init_policy(&mut fx, &foreign.pubkey(), &owner, 0, 0, 0, 0, DEFAULT_RECEIPT_TTL_SLOTS)
        .expect_err("policy target must be owned by this program");
}
