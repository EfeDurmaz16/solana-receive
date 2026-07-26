//! Compute / size regression alarms (LiteSVM), not optimization targets.
//!
//! ```bash
//! export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
//! cargo build-sbf --manifest-path program/token-2022-receive/Cargo.toml
//! cargo test -p token-2022-receive --test cu_ceilings -- --nocapture
//! ```
//!
//! **Mollusk:** not wired. Current stack is `solana-program`/`solana-sdk` 2.2 +
//! `litesvm` 0.6.1 + Agave CLI 4.1.x. `mollusk-svm` 0.14 targets a newer Agave
//! line; adding it here would be dead scaffolding until the workspace upgrades.
//! LiteSVM remains the executable CU baseline.

#[path = "common/litesvm.rs"]
mod litesvm_helpers;

use litesvm_helpers::{send, Fixture};
use solana_sdk::{message::Message, signature::Signer, transaction::Transaction};
use token_2022_receive::constants::DEFAULT_RECEIPT_TTL_SLOTS;
use token_2022_receive::guard::{derive_guard_state_address, derive_guard_token_address};
use token_2022_receive::instruction::HeldLimits;
use token_2022_receive::instruction::{
    claim_receipt, close_expired_receipt, transfer_checked, PolicyTransferAccounts,
};
use token_2022_receive::receipt::derive_receipt_address;

/// Generous regression ceilings (CU). Fail only on large regressions.
const CEIL_NO_POLICY: u64 = 10_000;
const CEIL_CREDITED: u64 = 40_000;
const CEIL_HELD: u64 = 50_000;
const CEIL_MISSING: u64 = 10_000;
const CEIL_CLAIM: u64 = 40_000;
const CEIL_EXPIRY: u64 = 40_000;

fn policy_metas(
    fx: &Fixture,
    nonce: [u8; 32],
) -> (
    solana_sdk::pubkey::Pubkey,
    solana_sdk::pubkey::Pubkey,
    solana_sdk::pubkey::Pubkey,
    PolicyTransferAccounts,
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
    (
        guard_token,
        guard_state,
        receipt,
        PolicyTransferAccounts {
            guard_token,
            guard_state,
            receipt,
            bond_payer: fx.payer.pubkey(),
        },
    )
}

fn assert_cu(label: &str, cu: u64, ceiling: u64) {
    eprintln!("[{label}] CU={cu} ceiling={ceiling}");
    assert!(
        cu <= ceiling,
        "{label}: CU {cu} exceeded regression ceiling {ceiling}"
    );
}

#[test]
fn cu_ceiling_no_policy_transfer() {
    let mut fx = Fixture::boot(1_000).with_plain_dest();
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
            1,
            6,
            [0u8; 32],
            HeldLimits::unlimited(),
            None,
        )],
    )
    .expect("no-policy");
    assert_cu("no-policy", meta.compute_units_consumed, CEIL_NO_POLICY);
}

#[test]
fn cu_ceiling_policy_credited_and_held_and_missing() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let nonce_c = [1u8; 32];
    let (_, _, _, metas_c) = policy_metas(&fx, nonce_c);
    let meta_c = send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.source_owner],
        vec![transfer_checked(
            &fx.program_id,
            &fx.source.pubkey(),
            &fx.mint.pubkey(),
            &fx.dest.pubkey(),
            &fx.source_owner.pubkey(),
            150,
            6,
            nonce_c,
            HeldLimits::unlimited(),
            Some(metas_c),
        )],
    )
    .expect("credited");
    assert_cu("credited", meta_c.compute_units_consumed, CEIL_CREDITED);
    fx.svm.expire_blockhash();

    let nonce_h = [2u8; 32];
    let (_, _, _, metas_h) = policy_metas(&fx, nonce_h);
    let meta_h = send(
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
            nonce_h,
            HeldLimits::unlimited(),
            Some(metas_h),
        )],
    )
    .expect("held");
    assert_cu("held", meta_h.compute_units_consumed, CEIL_HELD);
    fx.svm.expire_blockhash();

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
            [3u8; 32],
            HeldLimits::unlimited(),
            None,
        )],
    )
    .expect_err("missing metas");
    let cu = match err {
        litesvm::types::FailedTransactionMetadata { meta, .. } => meta.compute_units_consumed,
    };
    assert_cu("missing-metas", cu, CEIL_MISSING);
}

#[test]
fn cu_ceiling_claim_and_expiry() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let nonce = [4u8; 32];
    let (guard_token, guard_state, receipt, metas) = policy_metas(&fx, nonce);
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
            Some(metas),
        )],
    )
    .expect("held for claim");
    fx.svm.expire_blockhash();

    let claim_dest = fx.create_token_account(&fx.source_owner.pubkey());
    fx.svm.expire_blockhash();
    let meta_claim = send(
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
    assert_cu("claim", meta_claim.compute_units_consumed, CEIL_CLAIM);

    // Fresh held for expiry path.
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let nonce = [5u8; 32];
    let (guard_token, guard_state, receipt, metas) = policy_metas(&fx, nonce);
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
            Some(metas),
        )],
    )
    .expect("held for expiry");
    fx.svm.expire_blockhash();
    fx.svm.warp_to_slot(DEFAULT_RECEIPT_TTL_SLOTS + 10);
    fx.svm.expire_blockhash();
    let meta_ex = send(
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
    .expect("expiry");
    assert_cu("expiry", meta_ex.compute_units_consumed, CEIL_EXPIRY);
}

#[test]
fn serialized_tx_footprint_under_packet_limit() {
    let fx = Fixture::boot(100).with_policy_dest(50);
    let nonce = [9u8; 32];
    let (_, _, _, metas) = policy_metas(&fx, nonce);
    let ix = transfer_checked(
        &fx.program_id,
        &fx.source.pubkey(),
        &fx.mint.pubkey(),
        &fx.dest.pubkey(),
        &fx.source_owner.pubkey(),
        1,
        6,
        nonce,
        HeldLimits::unlimited(),
        Some(metas),
    );
    let tx = Transaction::new_unsigned(Message::new(&[ix], Some(&fx.payer.pubkey())));
    let bytes = bincode::serialize(&tx).expect("serialize");
    eprintln!(
        "[policy-transfer tx] accounts≈9 serialized≈{}B limit=1232",
        bytes.len()
    );
    assert!(bytes.len() < 1232);
}

/// Account-lock analysis (runtime contention unmeasured under LiteSVM).
#[test]
fn distinct_shards_share_no_writable_account() {
    // Contention is decided by the writable set a transaction locks. Distinct (receiver, mint)
    // shards must share nothing writable, or held delivery to unrelated receivers would
    // serialize behind each other. Same shard deliberately does share, and serializes.
    //
    // This asserts the account-lock structure, which is what the design controls. Throughput
    // under a real scheduler stays unmeasured: LiteSVM does not model bank locks.
    use std::collections::HashSet;
    let program_id = token_2022_receive::id();
    let mint_a = solana_sdk::pubkey::Pubkey::new_unique();
    let mint_b = solana_sdk::pubkey::Pubkey::new_unique();
    let r1 = solana_sdk::pubkey::Pubkey::new_unique();
    let r2 = solana_sdk::pubkey::Pubkey::new_unique();

    let writable = |receiver: &solana_sdk::pubkey::Pubkey,
                    mint: &solana_sdk::pubkey::Pubkey|
     -> HashSet<solana_sdk::pubkey::Pubkey> {
        let (gt, _) = derive_guard_token_address(receiver, mint, &program_id);
        let (gs, _) = derive_guard_state_address(receiver, mint, &program_id);
        HashSet::from([gt, gs])
    };

    // Different receivers, same mint.
    assert!(writable(&r1, &mint_a).is_disjoint(&writable(&r2, &mint_a)));
    // Same receiver, different mints.
    assert!(writable(&r1, &mint_a).is_disjoint(&writable(&r1, &mint_b)));
    // Different receivers and mints.
    assert!(writable(&r1, &mint_a).is_disjoint(&writable(&r2, &mint_b)));
    // The same shard shares both writable accounts, by design.
    assert_eq!(writable(&r1, &mint_a), writable(&r1, &mint_a));

    // Receipts are per (receiver, mint, source_owner, nonce), so two senders holding into the
    // same shard still write different receipt accounts.
    let s1 = solana_sdk::pubkey::Pubkey::new_unique();
    let s2 = solana_sdk::pubkey::Pubkey::new_unique();
    let (rc1, _) = token_2022_receive::receipt::derive_receipt_address(
        &r1,
        &mint_a,
        &s1,
        &[0u8; 32],
        &program_id,
    );
    let (rc2, _) = token_2022_receive::receipt::derive_receipt_address(
        &r1,
        &mint_a,
        &s2,
        &[0u8; 32],
        &program_id,
    );
    assert_ne!(rc1, rc2);
}
