//! Host verify: claim + expiry receipt lifecycle.

#[path = "common/host/mod.rs"]
mod host;

use host::{
    err_custom, run_claim, run_claim_ex, run_close_expired, run_close_expired_ex, ClaimCloseOpts,
};
use solana_program::pubkey::Pubkey;
use token_2022_receive::constants::DEFAULT_RECEIPT_TTL_SLOTS;
use token_2022_receive::error::ReceiveTokenError;

#[test]
fn host_claim_moves_guard_to_destination_and_closes_receipt() {
    let out = run_claim(77, 1_000_000, 10, 10 + DEFAULT_RECEIPT_TTL_SLOTS);
    out.result.unwrap();
    assert_eq!(out.guard_amt, 0);
    assert_eq!(out.dest_amt, 77);
    assert_eq!(out.bond_lamports, 1_000_000);
    assert_eq!(out.receipt_lamports, 0);
    assert_eq!(out.open_receipts, 0);
}

#[test]
fn host_claim_rejects_wrong_bond_destination() {
    let out = run_claim_ex(
        50,
        1_000_000,
        10,
        10 + DEFAULT_RECEIPT_TTL_SLOTS,
        ClaimCloseOpts {
            bond_dest_override: Some(Pubkey::new_unique()),
            ..ClaimCloseOpts::default()
        },
    );
    assert_eq!(
        out.result.unwrap_err(),
        err_custom(ReceiveTokenError::InvalidBondDestination)
    );
    assert_eq!(out.guard_amt, 50);
    assert_eq!(out.bond_lamports, 0);
    assert_eq!(out.receipt_lamports, 1_000_000);
}

#[test]
fn host_claim_rejects_wrong_guard_token_pda() {
    let out = run_claim_ex(
        50,
        1_000_000,
        10,
        10 + DEFAULT_RECEIPT_TTL_SLOTS,
        ClaimCloseOpts {
            guard_token_override: Some(Pubkey::new_unique()),
            ..ClaimCloseOpts::default()
        },
    );
    assert_eq!(
        out.result.unwrap_err(),
        err_custom(ReceiveTokenError::InvalidPda)
    );
    assert_eq!(out.guard_amt, 50);
}

#[test]
fn host_close_expired_returns_to_source_owner_ata() {
    let out = run_close_expired(55, 3, 2_000_000, 1_000, 10_000, 50_000);
    out.result.unwrap();
    assert_eq!(out.guard_amt, 0);
    assert_eq!(out.dest_amt, 58);
    assert_eq!(out.bond_lamports, 2_000_000);
    assert_eq!(out.receipt_lamports, 0);
}

#[test]
fn host_close_expired_rejects_wrong_bond_destination() {
    let out = run_close_expired_ex(
        55,
        3,
        2_000_000,
        1_000,
        10_000,
        50_000,
        ClaimCloseOpts {
            bond_dest_override: Some(Pubkey::new_unique()),
            ..ClaimCloseOpts::default()
        },
    );
    assert_eq!(
        out.result.unwrap_err(),
        err_custom(ReceiveTokenError::InvalidBondDestination)
    );
    assert_eq!(out.guard_amt, 55);
    assert_eq!(out.bond_lamports, 0);
    assert_eq!(out.receipt_lamports, 2_000_000);
}

#[test]
fn host_close_expired_before_ttl_fails() {
    let out = run_close_expired(10, 0, 1_000_000, 1_000, 10_000, 5_000);
    assert_eq!(
        out.result.unwrap_err(),
        err_custom(ReceiveTokenError::ReceiptNotExpired)
    );
    assert_eq!(out.guard_amt, 10);
}
