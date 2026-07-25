//! sRFC §9 golden vectors — executable matrix for the reference program.
//!
//! ## `unique_nonce` client contract
//! - Client supplies 32 bytes; receipt PDA seeds include
//!   `(receiver, mint, source_owner, unique_nonce)`.
//! - Distinct nonces → distinct receipt PDAs (no global writable nonce account).
//! - Reusing a nonce while a receipt account still holds data → `AlreadyInUse` / failed held create.
//!
//! Vector IDs match `docs/proposals/srfc-receive-policy-held-delivery.md` §9.

#[path = "common/host/mod.rs"]
mod host;

use host::{
    assert_held_receipt, err_custom, no_policy_transfer, policy_transfer, policy_transfer_ex,
    run_claim, run_claim_ex, run_close_expired, ClaimCloseOpts, PolicyTransferOpts,
};
use solana_program::pubkey::Pubkey;
use token_2022_receive::constants::{DEFAULT_RECEIPT_TTL_SLOTS, MAX_OPEN_RECEIPTS};
use token_2022_receive::error::ReceiveTokenError;
use token_2022_receive::extension::receive_policy::{ReceivePolicy, SourceOwnerMode};
use token_2022_receive::receipt::derive_receipt_address;

fn hold_policy(min_amount: u64) -> ReceivePolicy {
    let mut p = ReceivePolicy::default();
    p.min_amount = min_amount;
    p.receipt_bond_lamports = 1_000_000;
    p.receipt_ttl_slots = DEFAULT_RECEIPT_TTL_SLOTS;
    p
}

/// V-NP — No extension → ordinary credit.
#[test]
fn v_np_no_policy_credits_destination() {
    let (r, s, d) = no_policy_transfer(100, 0, 40, 6);
    r.unwrap();
    assert_eq!((s, d), (60, 40));
}

/// V-CR — Policy accept → credited.
#[test]
fn v_cr_policy_accept_credits_destination() {
    let out = policy_transfer(&hold_policy(10), 100, 50, [1u8; 32], true);
    out.result.unwrap();
    assert_eq!(out.dest_amt, 50);
    assert_eq!(out.guard_amt, 0);
    assert_eq!(out.open_receipts, 0);
}

/// V-HD — Policy reject → held.
#[test]
fn v_hd_policy_reject_holds_to_guard() {
    let out = policy_transfer(&hold_policy(100), 100, 1, [2u8; 32], true);
    out.result.as_ref().unwrap();
    // Default host stub slot is 1000; TTL from hold_policy.
    assert_held_receipt(&out, 1, 1000 + DEFAULT_RECEIPT_TTL_SLOTS);
    assert_eq!(out.dest_amt, 0);
    assert_eq!(out.guard_amt, 1);
}

/// V-FL — Missing metas → failed.
#[test]
fn v_fl_missing_metas_fails() {
    let out = policy_transfer(&hold_policy(100), 100, 1, [3u8; 32], false);
    assert_eq!(
        out.result.unwrap_err(),
        err_custom(ReceiveTokenError::MissingPolicyAccounts)
    );
    assert_eq!(out.source_amt, 100);
    assert_eq!(out.dest_amt, 0);
}

/// V-FL — Insufficient funds → failed.
#[test]
fn v_fl_insufficient_funds_fails() {
    let (r, s, d) = no_policy_transfer(10, 0, 11, 6);
    assert!(r.is_err());
    assert_eq!((s, d), (10, 0));
}

/// V-FL — Guard at capacity → failed (policy-reject would have held).
#[test]
fn v_fl_guard_at_capacity_fails() {
    let out = policy_transfer_ex(
        &hold_policy(100),
        100,
        1,
        [4u8; 32],
        PolicyTransferOpts {
            open_receipts: MAX_OPEN_RECEIPTS,
            ..PolicyTransferOpts::default()
        },
    );
    assert_eq!(
        out.result.unwrap_err(),
        err_custom(ReceiveTokenError::GuardAtCapacity)
    );
    assert_eq!(out.source_amt, 100);
    assert_eq!(out.dest_amt, 0);
    assert_eq!(out.guard_amt, 0);
}

/// V-CL — Authorized claim moves full amount.
#[test]
fn v_cl_claim_full_amount() {
    let out = run_claim(77, 1_000_000, 10, 10 + DEFAULT_RECEIPT_TTL_SLOTS);
    out.result.unwrap();
    assert_eq!(out.guard_amt, 0);
    assert_eq!(out.dest_amt, 77);
    assert_eq!(out.open_receipts, 0);
}

/// V-CL — Wrong claim authority → failed.
#[test]
fn v_cl_wrong_authority_fails() {
    let out = run_claim_ex(
        50,
        1_000_000,
        10,
        10 + DEFAULT_RECEIPT_TTL_SLOTS,
        ClaimCloseOpts {
            claim_authority_override: Some(Pubkey::new_unique()),
            ..ClaimCloseOpts::default()
        },
    );
    assert_eq!(
        out.result.unwrap_err(),
        err_custom(ReceiveTokenError::UnauthorizedClaim)
    );
    assert_eq!(out.guard_amt, 50);
}

/// V-EX — Close after TTL returns to source-owner ATA.
#[test]
fn v_ex_close_after_ttl() {
    let out = run_close_expired(55, 3, 2_000_000, 1_000, 10_000, 50_000);
    out.result.unwrap();
    assert_eq!(out.guard_amt, 0);
    assert_eq!(out.dest_amt, 58);
}

/// V-EX — Close before TTL → failed.
#[test]
fn v_ex_pre_ttl_fails() {
    let out = run_close_expired(10, 0, 1_000_000, 1_000, 10_000, 5_000);
    assert_eq!(
        out.result.unwrap_err(),
        err_custom(ReceiveTokenError::ReceiptNotExpired)
    );
}

/// V-AU — Allowlist membership is source owner (unit + transfer).
#[test]
fn v_au_allowlist_uses_source_owner() {
    let allowed = Pubkey::new_unique();
    let other = Pubkey::new_unique();
    let mut policy = hold_policy(1);
    policy.source_owner_mode = SourceOwnerMode::Allowlist as u8;
    policy.allowlist_len = 1;
    policy.allowlist[0] = allowed;

    assert!(policy.accepts(1, &allowed).unwrap());
    assert!(!policy.accepts(1, &other).unwrap());

    let credited = policy_transfer_ex(
        &policy,
        100,
        10,
        [5u8; 32],
        PolicyTransferOpts {
            source_owner: Some(allowed),
            ..PolicyTransferOpts::default()
        },
    );
    credited.result.unwrap();
    assert_eq!(credited.dest_amt, 10);
    assert_eq!(credited.guard_amt, 0);

    let held = policy_transfer_ex(
        &policy,
        100,
        10,
        [6u8; 32],
        PolicyTransferOpts {
            source_owner: Some(other),
            ..PolicyTransferOpts::default()
        },
    );
    held.result.unwrap();
    assert_eq!(held.dest_amt, 0);
    assert_eq!(held.guard_amt, 10);
}

/// unique_nonce — distinct nonces ⇒ distinct receipt PDAs; no global writable nonce.
#[test]
fn unique_nonce_distinct_pdas_no_global_account() {
    let program_id = token_2022_receive::id();
    let receiver = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let source_owner = Pubkey::new_unique();
    let (a, _) = derive_receipt_address(&receiver, &mint, &source_owner, &[1u8; 32], &program_id);
    let (b, _) = derive_receipt_address(&receiver, &mint, &source_owner, &[2u8; 32], &program_id);
    assert_ne!(a, b);
}

/// unique_nonce — collision / prefilled receipt ⇒ held create fails.
#[test]
fn unique_nonce_collision_fails_already_in_use() {
    let out = policy_transfer_ex(
        &hold_policy(100),
        100,
        1,
        [7u8; 32],
        PolicyTransferOpts {
            receipt_prefilled: true,
            ..PolicyTransferOpts::default()
        },
    );
    assert_eq!(
        out.result.unwrap_err(),
        err_custom(ReceiveTokenError::AlreadyInUse)
    );
    assert_eq!(out.source_amt, 100);
    assert_eq!(out.dest_amt, 0);
}
