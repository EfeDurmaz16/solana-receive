//! Host verify: no-policy TransferChecked paths.

#[path = "common/host/mod.rs"]
mod host;

use host::{err_custom, no_policy_transfer};
use token_2022_receive::error::ReceiveTokenError;

#[test]
fn host_no_policy_transfer_credits_normally() {
    let (result, src, dst) = no_policy_transfer(500, 10, 100, 6);
    result.unwrap();
    assert_eq!((src, dst), (400, 110));
}

#[test]
fn host_no_policy_insufficient_funds_fails_unchanged() {
    let (result, src, dst) = no_policy_transfer(50, 10, 100, 6);
    assert_eq!(
        result.unwrap_err(),
        err_custom(ReceiveTokenError::InsufficientFunds)
    );
    assert_eq!((src, dst), (50, 10));
}

#[test]
fn host_no_policy_decimals_mismatch_fails() {
    let (result, _, _) = no_policy_transfer(500, 0, 1, 9);
    assert_eq!(
        result.unwrap_err(),
        err_custom(ReceiveTokenError::MintDecimalsMismatch)
    );
}
