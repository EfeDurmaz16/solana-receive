//! Regression suite for sender-declared held limits.
//!
//! The destination writes the ReceivePolicy, but the sender pays for it: the bond is debited
//! from `bond_payer` and the TTL decides how long a rejected transfer stays locked. Protocol
//! caps bound the worst case; these limits let an individual sender state its own terms, and
//! refuse held delivery outright.

#[path = "common/litesvm.rs"]
mod litesvm_helpers;

use litesvm_helpers::{send, token_amount, Fixture};
use solana_sdk::signature::Signer;
use token_2022_receive::guard::{derive_guard_state_address, derive_guard_token_address};
use token_2022_receive::instruction::{transfer_checked, HeldLimits, PolicyTransferAccounts};
use token_2022_receive::receipt::derive_receipt_address;

/// dest carries ReceivePolicy { min_amount: 100 }, so 99 is rejected and would be held.
fn attempt(
    fx: &mut Fixture,
    limits: HeldLimits,
    nonce: [u8; 32],
) -> litesvm::types::TransactionResult {
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
    let r = send(
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
            limits,
            Some(PolicyTransferAccounts {
                guard_token,
                guard_state,
                receipt,
                bond_payer: fx.payer.pubkey(),
            }),
        )],
    );
    fx.svm.expire_blockhash();
    r
}

#[test]
fn a_sender_can_refuse_held_delivery_outright() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let before = token_amount(&fx.svm, &fx.source.pubkey());

    attempt(&mut fx, HeldLimits::no_hold(), [91u8; 32])
        .expect_err("no_hold turns a policy rejection into a failure, not a hold");

    // Nothing moved: this is the whole point, the sender keeps the funds.
    assert_eq!(token_amount(&fx.svm, &fx.source.pubkey()), before);
    assert_eq!(token_amount(&fx.svm, &guard_token), 0);
}

#[test]
fn a_sender_can_cap_the_bond_it_will_fund() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);

    // with_policy_dest sets receipt_bond_lamports = 1_000_000, and the bond is at least the
    // receipt's rent, so a 1 lamport ceiling must refuse.
    attempt(
        &mut fx,
        HeldLimits {
            max_bond_lamports: 1,
            max_ttl_slots: u64::MAX,
        },
        [92u8; 32],
    )
    .expect_err("a bond above the sender's ceiling must be refused");

    attempt(&mut fx, HeldLimits::unlimited(), [93u8; 32])
        .expect("the same transfer succeeds without a ceiling");
}

#[test]
fn a_sender_can_cap_how_long_its_funds_are_locked() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);

    // The fixture policy uses the 7-day default TTL.
    attempt(
        &mut fx,
        HeldLimits {
            max_bond_lamports: u64::MAX,
            max_ttl_slots: 1_000,
        },
        [94u8; 32],
    )
    .expect_err("a TTL above the sender's ceiling must be refused");

    attempt(
        &mut fx,
        HeldLimits {
            max_bond_lamports: u64::MAX,
            max_ttl_slots: token_2022_receive::constants::DEFAULT_RECEIPT_TTL_SLOTS,
        },
        [95u8; 32],
    )
    .expect("a TTL exactly at the ceiling is accepted");
}

#[test]
fn limits_do_not_affect_a_credited_transfer() {
    // Limits bound a held outcome only. A payment the policy accepts must not be refusable by
    // the sender's held terms.
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let nonce = [96u8; 32];
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
            150, // above min_amount -> credited
            6,
            nonce,
            HeldLimits::no_hold(),
            Some(PolicyTransferAccounts {
                guard_token,
                guard_state,
                receipt,
                bond_payer: fx.payer.pubkey(),
            }),
        )],
    )
    .expect("credited transfers ignore held limits");

    assert_eq!(token_amount(&fx.svm, &fx.dest.pubkey()), 150);
}
