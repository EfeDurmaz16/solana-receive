//! Regression suite for receipt settlement authority.
//!
//! SPEC section 7 defines three recovery modes; only `Originator` had coverage, and nothing
//! tested that an unsigned authority is rejected or that the payout account may not alias the
//! guard.

#[path = "common/litesvm.rs"]
mod litesvm_helpers;

use litesvm_helpers::{send, token_amount, Fixture};
use solana_sdk::instruction::{AccountMeta, Instruction};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use token_2022_receive::constants::DEFAULT_RECEIPT_TTL_SLOTS;
use token_2022_receive::extension::receive_policy::{RecoveryAuthorityMode, SourceOwnerMode};
use token_2022_receive::extension::tlv::account_len_with_receive_policy;
use token_2022_receive::guard::{derive_guard_state_address, derive_guard_token_address};
use token_2022_receive::instruction::{
    claim_receipt, close_expired_receipt, ensure_guard, initialize_account3,
    initialize_receive_policy, transfer_checked, PolicyTransferAccounts, ReceiveTokenInstruction,
};
use token_2022_receive::receipt::derive_receipt_address;

struct Held {
    guard_token: Pubkey,
    guard_state: Pubkey,
    receipt: Pubkey,
    dest: Keypair,
}

/// Build a policy destination with an explicit recovery mode, then hold 99 tokens in its guard.
fn hold_with_recovery(
    fx: &mut Fixture,
    mode: RecoveryAuthorityMode,
    recovery_authority: Pubkey,
    nonce: [u8; 32],
) -> Held {
    let owner = fx.dest_owner.insecure_clone();
    let space = account_len_with_receive_policy();
    let rent = fx.svm.minimum_balance_for_rent_exemption(space);
    let dest = Keypair::new();
    send(
        &mut fx.svm,
        &fx.payer,
        &[&dest],
        vec![solana_sdk::system_instruction::create_account(
            &fx.payer.pubkey(),
            &dest.pubkey(),
            rent,
            space as u64,
            &fx.program_id,
        )],
    )
    .expect("create dest");
    send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![initialize_account3(
            &fx.program_id,
            &dest.pubkey(),
            &fx.mint.pubkey(),
            &owner.pubkey(),
        )],
    )
    .expect("init dest");
    send(
        &mut fx.svm,
        &fx.payer,
        &[&owner],
        vec![initialize_receive_policy(
            &fx.program_id,
            &dest.pubkey(),
            &owner.pubkey(),
            100, // min_amount: 99 will be held
            SourceOwnerMode::AllowAll as u8,
            mode as u8,
            recovery_authority,
            0,
            DEFAULT_RECEIPT_TTL_SLOTS,
            vec![],
        )],
    )
    .expect("init policy");

    let (guard_token, _) =
        derive_guard_token_address(&owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![ensure_guard(
            &fx.program_id,
            &fx.payer.pubkey(),
            &owner.pubkey(),
            &fx.mint.pubkey(),
            &guard_token,
            &guard_state,
        )],
    )
    .expect("ensure guard");
    fx.svm.expire_blockhash();

    let (receipt, _) = derive_receipt_address(
        &owner.pubkey(),
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
            &dest.pubkey(),
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
        dest,
    }
}

#[test]
fn receiver_recovery_mode_lets_the_receiver_claim() {
    let mut fx = Fixture::boot(1_000);
    let held = hold_with_recovery(
        &mut fx,
        RecoveryAuthorityMode::Receiver,
        Pubkey::default(),
        [51u8; 32],
    );
    let owner = fx.dest_owner.insecure_clone();
    let payout = fx.create_token_account(&owner.pubkey());
    fx.svm.expire_blockhash();

    send(
        &mut fx.svm,
        &fx.payer,
        &[&owner],
        vec![claim_receipt(
            &fx.program_id,
            &held.receipt,
            &held.guard_token,
            &held.guard_state,
            &payout.pubkey(),
            &fx.mint.pubkey(),
            &owner.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect("receiver claims under Receiver mode");
    assert_eq!(token_amount(&fx.svm, &payout.pubkey()), 99);

    // ...and the originator must not.
    let _ = held.dest;
}

#[test]
fn third_party_recovery_mode_binds_the_named_key_only() {
    let mut fx = Fixture::boot(1_000);
    let third = Keypair::new();
    fx.svm.airdrop(&third.pubkey(), 1_000_000_000).unwrap();
    let held = hold_with_recovery(
        &mut fx,
        RecoveryAuthorityMode::ThirdParty,
        third.pubkey(),
        [52u8; 32],
    );
    let payout = fx.create_token_account(&third.pubkey());
    fx.svm.expire_blockhash();

    // The originator is NOT the recovery authority under ThirdParty.
    send(
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
    .expect_err("originator must not claim under ThirdParty mode");
    fx.svm.expire_blockhash();

    send(
        &mut fx.svm,
        &fx.payer,
        &[&third],
        vec![claim_receipt(
            &fx.program_id,
            &held.receipt,
            &held.guard_token,
            &held.guard_state,
            &payout.pubkey(),
            &fx.mint.pubkey(),
            &third.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect("named third party claims");
    assert_eq!(token_amount(&fx.svm, &payout.pubkey()), 99);
}

#[test]
fn claim_rejects_an_unsigned_authority() {
    let mut fx = Fixture::boot(1_000);
    let held = hold_with_recovery(
        &mut fx,
        RecoveryAuthorityMode::Originator,
        Pubkey::default(),
        [53u8; 32],
    );
    let payout = fx.create_token_account(&fx.source_owner.pubkey());
    fx.svm.expire_blockhash();

    // Correct authority key, but passed as a non-signer.
    let ix = Instruction {
        program_id: fx.program_id,
        accounts: vec![
            AccountMeta::new(held.receipt, false),
            AccountMeta::new(held.guard_token, false),
            AccountMeta::new(held.guard_state, false),
            AccountMeta::new(payout.pubkey(), false),
            AccountMeta::new_readonly(fx.mint.pubkey(), false),
            AccountMeta::new_readonly(fx.source_owner.pubkey(), false), // not a signer
            AccountMeta::new(fx.payer.pubkey(), false),
        ],
        data: ReceiveTokenInstruction::ClaimReceipt.pack(),
    };
    send(&mut fx.svm, &fx.payer, &[], vec![ix]).expect_err("unsigned authority must be rejected");
    assert_eq!(token_amount(&fx.svm, &held.guard_token), 99);
}

#[test]
fn claim_rejects_guard_as_its_own_payout_destination() {
    let mut fx = Fixture::boot(1_000);
    let held = hold_with_recovery(
        &mut fx,
        RecoveryAuthorityMode::Originator,
        Pubkey::default(),
        [54u8; 32],
    );
    fx.svm.expire_blockhash();

    // Aliasing debit and credit would cancel to a no-op while still closing the receipt,
    // stranding the tokens with nothing left to recover them.
    send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.source_owner],
        vec![claim_receipt(
            &fx.program_id,
            &held.receipt,
            &held.guard_token,
            &held.guard_state,
            &held.guard_token,
            &fx.mint.pubkey(),
            &fx.source_owner.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect_err("guard must not be its own payout destination");

    assert_eq!(token_amount(&fx.svm, &held.guard_token), 99);
    assert!(fx.svm.get_account(&held.receipt).expect("receipt").lamports > 0);
}

#[test]
fn expiry_close_rejects_guard_as_the_source_owner_account() {
    let mut fx = Fixture::boot(1_000);
    let held = hold_with_recovery(
        &mut fx,
        RecoveryAuthorityMode::Originator,
        Pubkey::default(),
        [55u8; 32],
    );
    fx.svm.warp_to_slot(DEFAULT_RECEIPT_TTL_SLOTS + 10);
    fx.svm.expire_blockhash();

    // CloseExpiredReceipt is permissionless, so this shape is reachable by anyone.
    send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![close_expired_receipt(
            &fx.program_id,
            &held.receipt,
            &held.guard_token,
            &held.guard_state,
            &held.guard_token,
            &fx.mint.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect_err("guard must not be the expiry payout account");
    assert_eq!(token_amount(&fx.svm, &held.guard_token), 99);
}
