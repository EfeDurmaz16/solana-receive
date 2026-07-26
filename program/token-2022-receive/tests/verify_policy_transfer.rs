//! Host verify: ReceivePolicy transfer credited / held / missing metas + ix footprint.

#[path = "common/host/mod.rs"]
mod host;

use host::{
    assert_held_receipt, err_custom, ix_account_counts, policy_ix_data_len, policy_transfer,
    set_slot, with_stubs,
};
use token_2022_receive::error::ReceiveTokenError;
use token_2022_receive::extension::receive_policy::{
    ReceivePolicy, RecoveryAuthorityMode, SourceOwnerMode,
};

#[test]
fn host_policy_credited_when_accepted() {
    with_stubs(|| {
        let mut policy = ReceivePolicy::default();
        policy.min_amount = 10;
        policy.source_owner_mode = SourceOwnerMode::AllowAll as u8;
        let out = policy_transfer(&policy, 500, 100, [3u8; 32], true);
        out.result.unwrap();
        assert_eq!(out.source_amt, 400);
        assert_eq!(out.dest_amt, 100);
        assert_eq!(out.guard_amt, 0);
        assert_eq!(out.receipt_lamports, 0);
        assert!(out.receipt_data.iter().all(|&b| b == 0));
    });
}

#[test]
fn host_policy_missing_metas_fails() {
    let mut policy = ReceivePolicy::default();
    policy.min_amount = 1;
    let out = policy_transfer(&policy, 500, 100, [0u8; 32], false);
    assert_eq!(
        out.result.unwrap_err(),
        err_custom(ReceiveTokenError::MissingPolicyAccounts)
    );
    assert_eq!((out.source_amt, out.dest_amt), (500, 0));
}

#[test]
fn host_policy_held_routes_to_guard_and_opens_receipt() {
    with_stubs(|| {
        set_slot(5_000);
        let mut policy = ReceivePolicy::default();
        policy.min_amount = 1_000;
        policy.source_owner_mode = SourceOwnerMode::AllowAll as u8;
        policy.recovery_authority_mode = RecoveryAuthorityMode::Originator as u8;
        policy.receipt_bond_lamports = 0;
        policy.receipt_ttl_slots = 100;
        let out = policy_transfer(&policy, 500, 100, [9u8; 32], true);
        out.result.as_ref().unwrap();
        assert_eq!(out.source_amt, 400);
        assert_eq!(out.dest_amt, 0);
        assert_eq!(out.guard_amt, 100);
        assert_held_receipt(&out, 100, 5_100);
    });
}

#[test]
fn host_policy_transfer_account_count_and_ix_footprint() {
    let (no_policy, policy, _, _) = ix_account_counts();
    assert_eq!(no_policy, 4);
    assert_eq!(policy, 9);
    assert_eq!(policy_ix_data_len(), 58);
    let approx = 32 + 9 * 34 + 58;
    assert!(approx < 1232);
}

#[test]
fn host_claim_and_expiry_account_counts() {
    let (_, _, claim, close) = ix_account_counts();
    assert_eq!(claim, 7);
    assert_eq!(close, 6);
}
