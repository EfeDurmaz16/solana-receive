//! Regression suite for accounts written by an earlier layout.
//!
//! This branch changes `GuardState` (112 to 128 bytes, gaining a version and `held_amount`) and
//! carves a `version` byte out of `Receipt`'s padding. The program has never been deployed, so no
//! such accounts exist and **no migration is provided**. What must hold is that an unreadable
//! account fails closed with a named error and is left exactly as it was: the alternative is
//! EnsureGuard repairing a vault while its companion state stays unloadable, which would seal the
//! balance under a shard where hold, claim and close all reject.

#[path = "common/litesvm.rs"]
mod litesvm_helpers;

use litesvm_helpers::{send, token_amount, Fixture};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use token_2022_receive::error::ReceiveTokenError;
use token_2022_receive::extension::tlv::{pack_account, unpack_account};
use token_2022_receive::guard::{
    derive_guard_state_address, derive_guard_token_address, GUARD_STATE_SIZE,
};
use token_2022_receive::instruction::{
    claim_receipt, close_expired_receipt, ensure_guard, transfer_checked, HeldLimits,
    PolicyTransferAccounts,
};
use token_2022_receive::receipt::{derive_receipt_address, RECEIPT_SIZE};

fn err_code(e: &litesvm::types::FailedTransactionMetadata) -> Option<u32> {
    match e.err {
        solana_sdk::transaction::TransactionError::InstructionError(
            _,
            solana_sdk::instruction::InstructionError::Custom(c),
        ) => Some(c),
        _ => None,
    }
}

struct Held {
    guard_token: Pubkey,
    guard_state: Pubkey,
    receipt: Pubkey,
}

fn hold(fx: &mut Fixture, nonce: [u8; 32]) -> Held {
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
            99,
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
    Held {
        guard_token,
        guard_state,
        receipt,
    }
}

/// Plant the pre-branch 112-byte GuardState: disc(8) receiver(32) mint(32) guard_token(32)
/// open_receipts u8(1) pad(7). No version, no held_amount, and every field after the
/// discriminator sits 8 bytes earlier than it does now.
fn plant_legacy_guard_state(fx: &mut Fixture, key: &Pubkey, receiver: &Pubkey, mint: &Pubkey) {
    let mut acct = fx.svm.get_account(key).expect("guard state");
    let mut legacy = vec![0u8; 112];
    legacy[..8]
        .copy_from_slice(&token_2022_receive::guard::GUARD_STATE_DISCRIMINATOR.to_le_bytes());
    legacy[8..40].copy_from_slice(receiver.as_ref());
    legacy[40..72].copy_from_slice(mint.as_ref());
    legacy[72..104].copy_from_slice(
        derive_guard_token_address(receiver, mint, &fx.program_id)
            .0
            .as_ref(),
    );
    legacy[104] = 1;
    acct.data = legacy;
    fx.svm.set_account(*key, acct).unwrap();
    fx.svm.expire_blockhash();
}

#[test]
fn ensure_guard_refuses_a_legacy_guard_state_and_leaves_the_vault_alone() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let held = hold(&mut fx, [131u8; 32]);
    assert_eq!(token_amount(&fx.svm, &held.guard_token), 99);

    let vault_before = fx.svm.get_account(&held.guard_token).unwrap().data.clone();
    let (receiver, mint) = (fx.dest_owner.pubkey(), fx.mint.pubkey());
    plant_legacy_guard_state(&mut fx, &held.guard_state, &receiver, &mint);

    let err = send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![ensure_guard(
            &fx.program_id,
            &fx.payer.pubkey(),
            &fx.dest_owner.pubkey(),
            &fx.mint.pubkey(),
            &held.guard_token,
            &held.guard_state,
        )],
    )
    .expect_err("an unreadable shard must be refused, not half-repaired");
    assert_eq!(
        err_code(&err),
        Some(ReceiveTokenError::UnsupportedStateVersion as u32),
        "and the reason must be named"
    );

    // The critical part: nothing was touched. A repaired vault plus an unloadable state would
    // seal the balance.
    assert_eq!(
        fx.svm.get_account(&held.guard_token).unwrap().data,
        vault_before,
        "vault left exactly as it was"
    );
    assert_eq!(
        fx.svm.get_account(&held.guard_state).unwrap().data.len(),
        112
    );
}

#[test]
fn a_legacy_guard_state_fails_the_held_path_closed() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let held = hold(&mut fx, [132u8; 32]);
    let (receiver, mint) = (fx.dest_owner.pubkey(), fx.mint.pubkey());
    plant_legacy_guard_state(&mut fx, &held.guard_state, &receiver, &mint);

    let (next_receipt, _) = derive_receipt_address(
        &fx.dest_owner.pubkey(),
        &fx.mint.pubkey(),
        &fx.source_owner.pubkey(),
        &[133u8; 32],
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
            99,
            6,
            [133u8; 32],
            HeldLimits::unlimited(),
            Some(PolicyTransferAccounts {
                guard_token: held.guard_token,
                guard_state: held.guard_state,
                receipt: next_receipt,
                bond_payer: fx.payer.pubkey(),
            }),
        )],
    )
    .expect_err("a hold into an unreadable shard must fail");
    assert_eq!(token_amount(&fx.svm, &held.guard_token), 99, "no deposit");
}

#[test]
fn a_version_zero_receipt_is_refused_by_both_settlement_paths() {
    // The version byte comes from what used to be padding, so a pre-branch receipt is the same
    // 304 bytes with version 0. Same size and same discriminator, so only the version check
    // separates them, and it must refuse rather than misread.
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let held = hold(&mut fx, [134u8; 32]);

    let mut acct = fx.svm.get_account(&held.receipt).expect("receipt");
    assert_eq!(acct.data.len(), RECEIPT_SIZE);
    // recovery_authority_mode, status, then version: offset 8 + 8 + 32*5 + 2 = 178.
    assert_eq!(acct.data[178], 1, "current receipts are version 1");
    acct.data[178] = 0;
    fx.svm.set_account(held.receipt, acct).unwrap();
    fx.svm.expire_blockhash();

    let payout = fx.create_token_account(&fx.source_owner.pubkey());
    fx.svm.expire_blockhash();
    let err = send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.source_owner],
        vec![claim_receipt(
            &fx.program_id,
            &held.receipt,
            &held.guard_token,
            &held.guard_state,
            &payout.pubkey(),
            &fx.mint.pubkey(),
            &fx.source_owner.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect_err("claim must refuse an unsupported receipt version");
    assert_eq!(
        err_code(&err),
        Some(ReceiveTokenError::UnsupportedStateVersion as u32)
    );
    fx.svm
        .warp_to_slot(token_2022_receive::constants::DEFAULT_RECEIPT_TTL_SLOTS + 10);
    fx.svm.expire_blockhash();

    let err = send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![close_expired_receipt(
            &fx.program_id,
            &held.receipt,
            &held.guard_token,
            &held.guard_state,
            &fx.source.pubkey(),
            &fx.mint.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect_err("expiry close must refuse it too");
    assert_eq!(
        err_code(&err),
        Some(ReceiveTokenError::UnsupportedStateVersion as u32)
    );
    assert_eq!(token_amount(&fx.svm, &payout.pubkey()), 0);
}

#[test]
fn guard_state_size_and_receipt_version_offset_are_pinned() {
    // The tests above hard-code the legacy 112-byte shape and the version byte at offset 178.
    // If either constant moves, they must be updated rather than silently testing nothing.
    assert_eq!(GUARD_STATE_SIZE, 128);
    assert_eq!(RECEIPT_SIZE, 304);
    let mut buf = vec![0u8; RECEIPT_SIZE];
    buf[178] = 7;
    let r: &token_2022_receive::receipt::Receipt = bytemuck::from_bytes(&buf);
    assert_eq!(r.version, 7, "version byte is at offset 178");
}

#[test]
fn a_repaired_vault_still_needs_a_readable_shard() {
    // Sanity: the vault repair from the previous round only runs once the shard validates, so
    // the two halves can never end up disagreeing.
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let held = hold(&mut fx, [135u8; 32]);

    let mut acct = fx.svm.get_account(&held.guard_token).expect("guard");
    let mut legacy = unpack_account(&acct.data).expect("unpack");
    legacy.owner = fx.dest_owner.pubkey();
    legacy.close_authority = solana_sdk::program_option::COption::None;
    pack_account(&legacy, &mut acct.data).unwrap();
    fx.svm.set_account(held.guard_token, acct).unwrap();
    let (receiver, mint) = (fx.dest_owner.pubkey(), fx.mint.pubkey());
    plant_legacy_guard_state(&mut fx, &held.guard_state, &receiver, &mint);

    send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![ensure_guard(
            &fx.program_id,
            &fx.payer.pubkey(),
            &fx.dest_owner.pubkey(),
            &fx.mint.pubkey(),
            &held.guard_token,
            &held.guard_state,
        )],
    )
    .expect_err("refused before the vault is touched");

    let after = unpack_account(&fx.svm.get_account(&held.guard_token).unwrap().data).unwrap();
    assert_eq!(after.owner, fx.dest_owner.pubkey(), "vault unchanged");
}
