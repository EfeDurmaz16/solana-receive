//! On-VM before/after checks via LiteSVM + compiled SBF.
//!
//! ```bash
//! export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
//! cargo build-sbf --manifest-path program/token-2022-receive/Cargo.toml
//! cargo test -p token-2022-receive --test litesvm_before_after -- --nocapture
//! ```

#[path = "common/litesvm.rs"]
mod litesvm_helpers;

use litesvm_helpers::{send, token_amount, Fixture};
use solana_sdk::signature::Signer;
use token_2022_receive::guard::{derive_guard_state_address, derive_guard_token_address};
use token_2022_receive::instruction::{transfer_checked, PolicyTransferAccounts};
use token_2022_receive::receipt::derive_receipt_address;

#[test]
fn before_no_policy_dust_credits_destination() {
    let mut fx = Fixture::boot(1_000).with_plain_dest();
    let dust = 1u64;
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
            dust,
            6,
            [0u8; 32],
            None,
        )],
    )
    .expect("BEFORE dust transfer must succeed");

    assert_eq!(token_amount(&fx.svm, &fx.source.pubkey()), 999);
    assert_eq!(token_amount(&fx.svm, &fx.dest.pubkey()), dust);
    eprintln!("[BEFORE no-policy dust] CU={}", meta.compute_units_consumed);
}

#[test]
fn after_policy_dust_is_held_not_credited() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let nonce = [9u8; 32];
    let (receipt, _) = derive_receipt_address(
        &fx.dest_owner.pubkey(),
        &fx.mint.pubkey(),
        &fx.source_owner.pubkey(),
        &nonce,
        &fx.program_id,
    );

    let dust = 1u64;
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
            dust,
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
    .expect("AFTER held path must succeed (non-reverting)");

    assert_eq!(token_amount(&fx.svm, &fx.source.pubkey()), 999);
    assert_eq!(token_amount(&fx.svm, &fx.dest.pubkey()), 0);
    assert_eq!(token_amount(&fx.svm, &guard_token), dust);
    assert!(!fx
        .svm
        .get_account(&receipt)
        .expect("receipt")
        .data
        .is_empty());
    eprintln!(
        "[AFTER policy held dust] CU={}",
        meta.compute_units_consumed
    );
}

#[test]
fn after_policy_accepted_credits_destination() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let nonce = [3u8; 32];
    let (receipt, _) = derive_receipt_address(
        &fx.dest_owner.pubkey(),
        &fx.mint.pubkey(),
        &fx.source_owner.pubkey(),
        &nonce,
        &fx.program_id,
    );

    let amount = 150u64;
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
    .expect("AFTER credited path");

    assert_eq!(token_amount(&fx.svm, &fx.source.pubkey()), 850);
    assert_eq!(token_amount(&fx.svm, &fx.dest.pubkey()), amount);
    assert_eq!(token_amount(&fx.svm, &guard_token), 0);
    assert!(fx
        .svm
        .get_account(&receipt)
        .map(|a| a.data.is_empty() && a.lamports == 0)
        .unwrap_or(true));
    eprintln!("[AFTER policy credited] CU={}", meta.compute_units_consumed);
}

#[test]
fn after_policy_missing_metas_fails() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
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
            1,
            6,
            [1u8; 32],
            None,
        )],
    )
    .expect_err("must fail when policy metas missing");

    assert_eq!(token_amount(&fx.svm, &fx.source.pubkey()), 1_000);
    assert_eq!(token_amount(&fx.svm, &fx.dest.pubkey()), 0);
    eprintln!("[AFTER missing metas] err={err:?}");
}
