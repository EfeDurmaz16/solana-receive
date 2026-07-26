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
            ..HeldLimits::unlimited()
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
            max_ttl_slots: 1_000,
            ..HeldLimits::unlimited()
        },
        [94u8; 32],
    )
    .expect_err("a TTL above the sender's ceiling must be refused");

    attempt(
        &mut fx,
        HeldLimits {
            max_ttl_slots: token_2022_receive::constants::DEFAULT_RECEIPT_TTL_SLOTS,
            ..HeldLimits::unlimited()
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

#[test]
fn a_sender_can_require_that_recovery_stays_with_it() {
    // Capping cost is not enough: under Receiver / ThirdParty recovery the party that rejected
    // the payment also chooses who may claim it back.
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    // with_policy_dest uses recovery_authority_mode = 0 (Originator), so this must pass.
    attempt(&mut fx, HeldLimits::originator_recovery_only(), [97u8; 32])
        .expect("Originator recovery is within the sender's terms");
}

#[test]
fn a_sender_refuses_a_policy_that_hands_recovery_to_the_receiver() {
    use token_2022_receive::extension::receive_policy::{RecoveryAuthorityMode, SourceOwnerMode};
    use token_2022_receive::instruction::{initialize_account3, initialize_receive_policy};

    let mut fx = Fixture::boot(1_000).with_plain_dest();
    let owner = fx.dest_owner.insecure_clone();
    let space = token_2022_receive::extension::tlv::account_len_with_receive_policy();
    let rent = fx.svm.minimum_balance_for_rent_exemption(space);
    let dest = solana_sdk::signature::Keypair::new();
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
            100,
            SourceOwnerMode::AllowAll as u8,
            RecoveryAuthorityMode::Receiver as u8, // the receiver claims what it rejects
            solana_sdk::pubkey::Pubkey::default(),
            0,
            token_2022_receive::constants::DEFAULT_RECEIPT_TTL_SLOTS,
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
        vec![token_2022_receive::instruction::ensure_guard(
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

    let nonce = [98u8; 32];
    let (receipt, _) = derive_receipt_address(
        &owner.pubkey(),
        &fx.mint.pubkey(),
        &fx.source_owner.pubkey(),
        &nonce,
        &fx.program_id,
    );
    let attempt_with = |fx: &mut Fixture, limits: HeldLimits, nonce: [u8; 32]| {
        let (receipt, _) = derive_receipt_address(
            &owner.pubkey(),
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
                &dest.pubkey(),
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
    };
    let _ = receipt;

    attempt_with(&mut fx, HeldLimits::originator_recovery_only(), nonce)
        .expect_err("a sender must be able to refuse handing recovery to the receiver");
    attempt_with(&mut fx, HeldLimits::unlimited(), [99u8; 32])
        .expect("the same transfer is accepted by a sender that allows it");
    assert_eq!(token_amount(&fx.svm, &guard_token), 99);
}

#[test]
fn the_sender_bond_ceiling_is_compared_against_the_rent_floored_bond() {
    // On chain the bond is max(policy.receiptBondLamports, rent(RECEIPT_SIZE)). A ceiling below
    // the rent floor must refuse the hold even when the policy asks for zero, or a sender would
    // be charged rent it never agreed to.
    let mut fx = Fixture::boot(1_000);
    let owner = fx.dest_owner.insecure_clone();
    let rent = fx
        .svm
        .minimum_balance_for_rent_exemption(token_2022_receive::receipt::RECEIPT_SIZE);
    assert!(rent > 0);

    // Policy asks for a zero bond, so only the rent floor is in play.
    let acct = {
        let space = token_2022_receive::extension::tlv::account_len_with_receive_policy();
        let r = fx.svm.minimum_balance_for_rent_exemption(space);
        let k = solana_sdk::signature::Keypair::new();
        send(
            &mut fx.svm,
            &fx.payer,
            &[&k],
            vec![solana_sdk::system_instruction::create_account(
                &fx.payer.pubkey(),
                &k.pubkey(),
                r,
                space as u64,
                &fx.program_id,
            )],
        )
        .expect("create");
        send(
            &mut fx.svm,
            &fx.payer,
            &[],
            vec![token_2022_receive::instruction::initialize_account3(
                &fx.program_id,
                &k.pubkey(),
                &fx.mint.pubkey(),
                &owner.pubkey(),
            )],
        )
        .expect("init");
        send(
            &mut fx.svm,
            &fx.payer,
            &[&owner],
            vec![token_2022_receive::instruction::initialize_receive_policy(
                &fx.program_id,
                &k.pubkey(),
                &owner.pubkey(),
                100,
                0,
                0,
                solana_sdk::pubkey::Pubkey::default(),
                0, // zero bond
                token_2022_receive::constants::DEFAULT_RECEIPT_TTL_SLOTS,
                vec![],
            )],
        )
        .expect("init policy");
        k
    };
    let (guard_token, _) =
        derive_guard_token_address(&owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![token_2022_receive::instruction::ensure_guard(
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

    let try_hold = |fx: &mut Fixture, max_bond: u64, nonce: [u8; 32]| {
        let (receipt, _) = derive_receipt_address(
            &owner.pubkey(),
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
                &acct.pubkey(),
                &fx.source_owner.pubkey(),
                99,
                6,
                nonce,
                HeldLimits {
                    max_bond_lamports: max_bond,
                    ..HeldLimits::unlimited()
                },
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
    };

    // A ceiling just under the rent floor must refuse, despite the policy asking for zero.
    try_hold(&mut fx, rent - 1, [141u8; 32])
        .expect_err("the rent floor counts toward the sender's bond ceiling");
    // Exactly at the floor is acceptable.
    try_hold(&mut fx, rent, [142u8; 32]).expect("a ceiling that covers rent is enough");
    assert_eq!(token_amount(&fx.svm, &guard_token), 99);
}
