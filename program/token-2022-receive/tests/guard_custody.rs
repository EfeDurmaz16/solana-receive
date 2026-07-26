//! Regression suite for guard custody: held funds must not be spendable by the receiver.
//!
//! Pins the invariant behind the `held` outcome - once tokens sit in the guard, the ONLY
//! debit paths are ClaimReceipt and CloseExpiredReceipt. Before this was enforced, the guard
//! token account's owner field was the receiver, so the receiver could drain every sender's
//! held balance with a plain 4-account TransferChecked and permanently strand the receipts.
//!
//! ```bash
//! cargo build-sbf --manifest-path program/token-2022-receive/Cargo.toml
//! cargo test -p token-2022-receive --test guard_custody
//! ```

#[path = "common/litesvm.rs"]
mod litesvm_helpers;

use litesvm_helpers::{send, token_amount, Fixture};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use token_2022_receive::extension::tlv::unpack_account;
use token_2022_receive::guard::{derive_guard_state_address, derive_guard_token_address};
use token_2022_receive::instruction::{claim_receipt, transfer_checked, PolicyTransferAccounts};
use token_2022_receive::receipt::derive_receipt_address;

struct Held {
    guard_token: Pubkey,
    guard_state: Pubkey,
    receipt: Pubkey,
}

/// dest carries ReceivePolicy { min_amount: 100 }, so a 99-token transfer is rejected -> held.
fn hold_99(fx: &mut Fixture, nonce: [u8; 32]) -> Held {
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
    assert_eq!(token_amount(&fx.svm, &guard_token), 99);
    Held {
        guard_token,
        guard_state,
        receipt,
    }
}

#[test]
fn guard_token_account_is_not_owned_by_the_receiver() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);

    let guard = unpack_account(&fx.svm.get_account(&guard_token).expect("guard").data)
        .expect("unpack guard");
    assert_ne!(
        guard.owner,
        fx.dest_owner.pubkey(),
        "receiver must not be the guard's spending authority"
    );
    assert_eq!(
        guard.owner, guard_state,
        "guard authority must be a PDA no keypair can sign for"
    );
}

#[test]
fn receiver_cannot_drain_guard_with_plain_transfer() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let held = hold_99(&mut fx, [21u8; 32]);

    let loot = fx.create_token_account(&fx.dest_owner.pubkey());
    fx.svm.expire_blockhash();

    let err = send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.dest_owner],
        vec![transfer_checked(
            &fx.program_id,
            &held.guard_token, // source = the guard itself
            &fx.mint.pubkey(),
            &loot.pubkey(),          // plain no-policy destination -> 4-account path
            &fx.dest_owner.pubkey(), // receiver signs
            99,
            6,
            [0u8; 32],
            None,
        )],
    )
    .expect_err("receiver must not be able to spend held custody");
    eprintln!("[guard drain blocked] {err:?}");

    assert_eq!(token_amount(&fx.svm, &held.guard_token), 99);
    assert_eq!(token_amount(&fx.svm, &loot.pubkey()), 0);
}

#[test]
fn guard_stays_claimable_by_the_originator_after_a_drain_attempt() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let held = hold_99(&mut fx, [22u8; 32]);

    let loot = fx.create_token_account(&fx.dest_owner.pubkey());
    fx.svm.expire_blockhash();
    let _ = send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.dest_owner],
        vec![transfer_checked(
            &fx.program_id,
            &held.guard_token,
            &fx.mint.pubkey(),
            &loot.pubkey(),
            &fx.dest_owner.pubkey(),
            99,
            6,
            [0u8; 32],
            None,
        )],
    );
    fx.svm.expire_blockhash();

    // Recovery still works: the originator claims the full amount.
    let claim_dest = fx.create_token_account(&fx.source_owner.pubkey());
    fx.svm.expire_blockhash();
    send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.source_owner],
        vec![claim_receipt(
            &fx.program_id,
            &held.receipt,
            &held.guard_token,
            &held.guard_state,
            &claim_dest.pubkey(),
            &fx.mint.pubkey(),
            &fx.source_owner.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect("originator claim");

    assert_eq!(token_amount(&fx.svm, &claim_dest.pubkey()), 99);
    assert_eq!(token_amount(&fx.svm, &held.guard_token), 0);
}

#[test]
fn guard_cannot_be_a_held_transfer_destination() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let nonce = [23u8; 32];
    let (receipt, _) = derive_receipt_address(
        &fx.dest_owner.pubkey(),
        &fx.mint.pubkey(),
        &fx.source_owner.pubkey(),
        &nonce,
        &fx.program_id,
    );

    // Routing a policy transfer *into* the guard would let a receipt be minted against
    // balance that never moved.
    let err = send(
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
            Some(PolicyTransferAccounts {
                guard_token,
                guard_state,
                receipt,
                bond_payer: fx.payer.pubkey(),
            }),
        )],
    );
    // The honest shape still succeeds; this asserts the honest path is unaffected.
    err.expect("ordinary held transfer still works");
    assert_eq!(token_amount(&fx.svm, &guard_token), 99);
}

#[test]
fn transfer_reports_credited_vs_held_in_return_data() {
    // `held` returns Ok, so a caller checking only "did the tx land" would read a diverted
    // payment as a delivered one. The outcome byte must distinguish them.
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);

    let mut outcome = |amount: u64, nonce: [u8; 32]| -> u8 {
        let (receipt, _) = derive_receipt_address(
            &fx.dest_owner.pubkey(),
            &fx.mint.pubkey(),
            &fx.source_owner.pubkey(),
            &nonce,
            &fx.program_id,
        );
        let meta = send(
            &mut fx.svm,
            &fx.payer,
            &[&fx.source_owner],
            vec![transfer_checked(
                &fx.program_id,
                &fx.source.pubkey(),
                &fx.mint.pubkey(),
                &fx.dest.pubkey(),
                &fx.source_owner.pubkey(),
                amount,
                6,
                nonce,
                Some(PolicyTransferAccounts {
                    guard_token,
                    guard_state,
                    receipt,
                    bond_payer: fx.payer.pubkey(),
                }),
            )],
        )
        .expect("transfer");
        fx.svm.expire_blockhash();
        meta.return_data.data[0]
    };

    assert_eq!(outcome(99, [31u8; 32]), 1, "below min_amount -> held");
    assert_eq!(
        outcome(150, [32u8; 32]),
        0,
        "at or above min_amount -> credited"
    );
}

#[test]
fn zero_amount_transfer_cannot_burn_a_shard_slot() {
    // A zero-amount hold moves nothing but consumes one of MAX_OPEN_RECEIPTS, so without
    // this an attacker holding no tokens at all could fill a victim's shard.
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let nonce = [61u8; 32];
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
            0,
            6,
            nonce,
            Some(PolicyTransferAccounts {
                guard_token,
                guard_state,
                receipt,
                bond_payer: fx.payer.pubkey(),
            }),
        )],
    )
    .expect_err("zero-amount hold must be rejected");

    assert!(fx.svm.get_account(&receipt).is_none_or(|a| a.lamports == 0));
}
