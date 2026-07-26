//! LiteSVM claim / expiry lifecycle (+ CU) against compiled SBF.
//!
//! ```bash
//! export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
//! cargo build-sbf --manifest-path program/token-2022-receive/Cargo.toml
//! cargo test -p token-2022-receive --test litesvm_lifecycle -- --nocapture
//! ```

#[path = "common/litesvm.rs"]
mod litesvm_helpers;

use litesvm_helpers::{send, token_amount, Fixture};
use solana_sdk::signature::Signer;
use token_2022_receive::constants::DEFAULT_RECEIPT_TTL_SLOTS;
use token_2022_receive::error::ReceiveTokenError;
use token_2022_receive::guard::{derive_guard_state_address, derive_guard_token_address};
use token_2022_receive::instruction::HeldLimits;
use token_2022_receive::instruction::{
    claim_receipt, close_expired_receipt, transfer_checked, PolicyTransferAccounts,
};
use token_2022_receive::receipt::derive_receipt_address;

fn held_dust(
    fx: &mut Fixture,
    nonce: [u8; 32],
) -> (
    solana_sdk::pubkey::Pubkey,
    solana_sdk::pubkey::Pubkey,
    solana_sdk::pubkey::Pubkey,
) {
    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (receipt, _) = derive_receipt_address(
        &fx.dest_owner.pubkey(),
        &fx.mint.pubkey(),
        &fx.source_owner.pubkey(),
        &nonce,
        &fx.program_id,
    );

    send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.source_owner],
        vec![transfer_checked(
            &fx.program_id,
            &fx.source.pubkey(),
            &fx.mint.pubkey(),
            &fx.dest.pubkey(),
            &fx.source_owner.pubkey(),
            1,
            6,
            nonce,
            HeldLimits::unlimited(),
            Some(PolicyTransferAccounts {
                guard_token,
                guard_state,
                receipt,
                bond_payer: fx.payer.pubkey(),
            }),
        )],
    )
    .expect("held transfer");
    fx.svm.expire_blockhash();

    assert_eq!(token_amount(&fx.svm, &fx.dest.pubkey()), 0);
    assert_eq!(token_amount(&fx.svm, &guard_token), 1);
    assert!(fx.svm.get_account(&receipt).expect("receipt").lamports > 0);
    (guard_token, guard_state, receipt)
}

fn err_code(e: &litesvm::types::FailedTransactionMetadata) -> Option<u32> {
    match e.err {
        solana_sdk::transaction::TransactionError::InstructionError(
            _,
            solana_sdk::instruction::InstructionError::Custom(c),
        ) => Some(c),
        _ => None,
    }
}

#[test]
fn litesvm_claim_moves_held_tokens_and_refunds_bond() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, guard_state, receipt) = held_dust(&mut fx, [11u8; 32]);
    let receipt_bond = fx.svm.get_account(&receipt).expect("receipt").lamports;
    let claim_dest = fx.create_token_account(&fx.source_owner.pubkey());
    fx.svm.expire_blockhash();
    let payer_before = fx
        .svm
        .get_account(&fx.payer.pubkey())
        .expect("payer")
        .lamports;

    let meta = send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.source_owner],
        vec![claim_receipt(
            &fx.program_id,
            &receipt,
            &guard_token,
            &guard_state,
            &claim_dest.pubkey(),
            &fx.mint.pubkey(),
            &fx.source_owner.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect("claim");

    assert_eq!(token_amount(&fx.svm, &guard_token), 0);
    assert_eq!(token_amount(&fx.svm, &claim_dest.pubkey()), 1);
    let receipt_acc = fx.svm.get_account(&receipt);
    assert!(receipt_acc
        .as_ref()
        .map(|a| a.lamports == 0)
        .unwrap_or(true));
    let payer_after = fx
        .svm
        .get_account(&fx.payer.pubkey())
        .expect("payer")
        .lamports;
    // LiteSVM charges signature fees from the fee payer; bond refund must still net positive.
    assert!(
        payer_after + 20_000 >= payer_before + receipt_bond,
        "bond refund missing: before={payer_before} after={payer_after} bond={receipt_bond}"
    );
    assert!(
        payer_after > payer_before,
        "payer should receive receipt bond"
    );
    eprintln!("[claim held dust] CU={}", meta.compute_units_consumed);
}

#[test]
fn litesvm_claim_refunds_bond_payer_even_when_fee_payer_differs() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let bond_payer = solana_sdk::signature::Keypair::new();
    fx.svm
        .airdrop(&bond_payer.pubkey(), 10_000_000_000)
        .unwrap();

    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let nonce = [16u8; 32];
    let (receipt, _) = derive_receipt_address(
        &fx.dest_owner.pubkey(),
        &fx.mint.pubkey(),
        &fx.source_owner.pubkey(),
        &nonce,
        &fx.program_id,
    );

    send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.source_owner, &bond_payer],
        vec![transfer_checked(
            &fx.program_id,
            &fx.source.pubkey(),
            &fx.mint.pubkey(),
            &fx.dest.pubkey(),
            &fx.source_owner.pubkey(),
            1,
            6,
            nonce,
            HeldLimits::unlimited(),
            Some(PolicyTransferAccounts {
                guard_token,
                guard_state,
                receipt,
                bond_payer: bond_payer.pubkey(),
            }),
        )],
    )
    .expect("held transfer paid by separate bond payer");
    fx.svm.expire_blockhash();

    let receipt_bond = fx.svm.get_account(&receipt).expect("receipt").lamports;
    let claim_dest = fx.create_token_account(&fx.source_owner.pubkey());
    fx.svm.expire_blockhash();
    let bond_payer_before = fx
        .svm
        .get_account(&bond_payer.pubkey())
        .expect("bond payer")
        .lamports;
    let fee_payer_before = fx
        .svm
        .get_account(&fx.payer.pubkey())
        .expect("fee payer")
        .lamports;

    send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.source_owner],
        vec![claim_receipt(
            &fx.program_id,
            &receipt,
            &guard_token,
            &guard_state,
            &claim_dest.pubkey(),
            &fx.mint.pubkey(),
            &fx.source_owner.pubkey(),
            &bond_payer.pubkey(),
        )],
    )
    .expect("claim refunds recorded bond payer");

    assert_eq!(token_amount(&fx.svm, &guard_token), 0);
    assert_eq!(token_amount(&fx.svm, &claim_dest.pubkey()), 1);
    let bond_payer_after = fx
        .svm
        .get_account(&bond_payer.pubkey())
        .expect("bond payer")
        .lamports;
    let fee_payer_after = fx
        .svm
        .get_account(&fx.payer.pubkey())
        .expect("fee payer")
        .lamports;
    assert_eq!(
        bond_payer_after,
        bond_payer_before + receipt_bond,
        "receipt bond must return to the recorded bond payer"
    );
    assert!(
        fee_payer_after <= fee_payer_before,
        "fee payer must not receive the bond refund"
    );
}

#[test]
fn litesvm_claim_rejects_wrong_authority() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, guard_state, receipt) = held_dust(&mut fx, [12u8; 32]);
    let claim_dest = fx.create_token_account(&fx.source_owner.pubkey());
    fx.svm.expire_blockhash();

    let err = send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.dest_owner],
        vec![claim_receipt(
            &fx.program_id,
            &receipt,
            &guard_token,
            &guard_state,
            &claim_dest.pubkey(),
            &fx.mint.pubkey(),
            &fx.dest_owner.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect_err("receiver cannot claim under Originator recovery");

    assert_eq!(token_amount(&fx.svm, &guard_token), 1);
    eprintln!("[claim wrong auth] err={err:?}");
}

#[test]
fn litesvm_claim_rejects_wrong_bond_destination() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, guard_state, receipt) = held_dust(&mut fx, [13u8; 32]);
    let claim_dest = fx.create_token_account(&fx.source_owner.pubkey());
    let thief = solana_sdk::signature::Keypair::new();
    fx.svm.airdrop(&thief.pubkey(), 1_000_000).unwrap();
    fx.svm.expire_blockhash();

    let err = send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.source_owner],
        vec![claim_receipt(
            &fx.program_id,
            &receipt,
            &guard_token,
            &guard_state,
            &claim_dest.pubkey(),
            &fx.mint.pubkey(),
            &fx.source_owner.pubkey(),
            &thief.pubkey(),
        )],
    )
    .expect_err("bond cannot be redirected");

    assert_eq!(token_amount(&fx.svm, &guard_token), 1);
    assert!(fx.svm.get_account(&receipt).expect("receipt").lamports > 0);
    eprintln!("[claim wrong bond dest] err={err:?}");
}

#[test]
fn litesvm_expiry_returns_to_source_ata_after_ttl() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, guard_state, receipt) = held_dust(&mut fx, [14u8; 32]);
    let source_before = token_amount(&fx.svm, &fx.source.pubkey());

    send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![close_expired_receipt(
            &fx.program_id,
            &receipt,
            &guard_token,
            &guard_state,
            &fx.source.pubkey(),
            &fx.mint.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect_err("must fail before TTL");
    fx.svm.expire_blockhash();

    fx.svm.warp_to_slot(DEFAULT_RECEIPT_TTL_SLOTS + 10);
    fx.svm.expire_blockhash();

    let meta = send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![close_expired_receipt(
            &fx.program_id,
            &receipt,
            &guard_token,
            &guard_state,
            &fx.source.pubkey(),
            &fx.mint.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect("close expired");

    assert_eq!(token_amount(&fx.svm, &guard_token), 0);
    assert_eq!(
        token_amount(&fx.svm, &fx.source.pubkey()),
        source_before + 1
    );
    eprintln!("[close expired] CU={}", meta.compute_units_consumed);
}

#[test]
fn litesvm_expiry_rejects_same_mint_ata_owned_by_a_stranger() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, guard_state, receipt) = held_dust(&mut fx, [17u8; 32]);
    let stranger = solana_sdk::signature::Keypair::new();
    let stranger_ata = fx.create_token_account(&stranger.pubkey());
    fx.svm.expire_blockhash();

    fx.svm.warp_to_slot(DEFAULT_RECEIPT_TTL_SLOTS + 10);
    fx.svm.expire_blockhash();

    let err = send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![close_expired_receipt(
            &fx.program_id,
            &receipt,
            &guard_token,
            &guard_state,
            &stranger_ata.pubkey(),
            &fx.mint.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect_err("expiry can only return to the source owner's same-mint account");
    assert_eq!(
        err_code(&err),
        Some(ReceiveTokenError::OwnerMismatch as u32)
    );
    assert_eq!(token_amount(&fx.svm, &guard_token), 1);
    assert_eq!(token_amount(&fx.svm, &stranger_ata.pubkey()), 0);
    assert!(fx.svm.get_account(&receipt).expect("receipt").lamports > 0);
}

#[test]
fn litesvm_claim_replay_fails() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, guard_state, receipt) = held_dust(&mut fx, [15u8; 32]);
    let claim_dest = fx.create_token_account(&fx.source_owner.pubkey());
    fx.svm.expire_blockhash();

    send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.source_owner],
        vec![claim_receipt(
            &fx.program_id,
            &receipt,
            &guard_token,
            &guard_state,
            &claim_dest.pubkey(),
            &fx.mint.pubkey(),
            &fx.source_owner.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect("first claim");
    fx.svm.expire_blockhash();

    let err = send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.source_owner],
        vec![claim_receipt(
            &fx.program_id,
            &receipt,
            &guard_token,
            &guard_state,
            &claim_dest.pubkey(),
            &fx.mint.pubkey(),
            &fx.source_owner.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect_err("replay must fail");

    assert_eq!(token_amount(&fx.svm, &claim_dest.pubkey()), 1);
    eprintln!("[claim replay] err={err:?}");
}
