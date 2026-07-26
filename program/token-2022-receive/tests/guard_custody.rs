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
use token_2022_receive::instruction::HeldLimits;
use token_2022_receive::instruction::{claim_receipt, transfer_checked, PolicyTransferAccounts};
use token_2022_receive::receipt::derive_receipt_address;

struct Held {
    guard_token: Pubkey,
    guard_state: Pubkey,
    receipt: Pubkey,
}

/// dest carries ReceivePolicy { min_amount: 100 }, so a 99-token transfer is rejected -> held.
fn hold_99(fx: &mut Fixture, nonce: [u8; 32]) -> Held {
    let before = fx
        .svm
        .get_account(
            &derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id)
                .0,
        )
        .map(|a| unpack_account(&a.data).map(|t| t.amount).unwrap_or(0))
        .unwrap_or(0);
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
            HeldLimits::unlimited(),
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
    assert_eq!(token_amount(&fx.svm, &guard_token), before + 99);
    Held {
        guard_token,
        guard_state,
        receipt,
    }
}

#[test]
fn guard_token_account_is_not_owned_by_the_receiver() {
    let fx = Fixture::boot(1_000).with_policy_dest(100);
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
            HeldLimits::unlimited(),
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
            HeldLimits::unlimited(),
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
fn a_guard_cannot_be_credited_by_any_path() {
    // Tokens that reach a guard outside the held path have no receipt, so neither claim nor
    // expiry can ever move them out: they are destroyed. Before this was enforced, a plain
    // 4-account transfer naming the guard as destination succeeded AND reported outcome byte
    // 0 (credited) - the very byte integrators are told to trust.
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);

    // No-policy path (a guard never carries a policy, so this is the reachable shape).
    send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.source_owner],
        vec![transfer_checked(
            &fx.program_id,
            &fx.source.pubkey(),
            &fx.mint.pubkey(),
            &guard_token,
            &fx.source_owner.pubkey(),
            10,
            6,
            [0u8; 32],
            HeldLimits::unlimited(),
            None,
        )],
    )
    .expect_err("a guard must not be a transfer destination");
    fx.svm.expire_blockhash();

    // MintTo is the other way to credit an arbitrary token account.
    send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.mint_authority],
        vec![token_2022_receive::instruction::mint_to(
            &fx.program_id,
            &fx.mint.pubkey(),
            &guard_token,
            &fx.mint_authority.pubkey(),
            10,
        )],
    )
    .expect_err("a guard must not be a MintTo target");

    assert_eq!(token_amount(&fx.svm, &guard_token), 0);
}

#[test]
fn held_delivery_still_credits_the_guard() {
    // The guard refusal must not break the one path that is supposed to fund it.
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let held = hold_99(&mut fx, [81u8; 32]);
    assert_eq!(token_amount(&fx.svm, &held.guard_token), 99);
}

#[test]
fn held_transfer_requires_an_initialized_guard_state() {
    // load_guard_state validates the guard_state contents, not just its address: an
    // uninitialized or mismatched shard must not be silently incremented.
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
        vec![token_2022_receive::instruction::initialize_account3(
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
        vec![token_2022_receive::instruction::initialize_receive_policy(
            &fx.program_id,
            &dest.pubkey(),
            &owner.pubkey(),
            100,
            0,
            0,
            Pubkey::default(),
            0,
            token_2022_receive::constants::DEFAULT_RECEIPT_TTL_SLOTS,
            vec![],
        )],
    )
    .expect("init policy");
    fx.svm.expire_blockhash();

    // EnsureGuard deliberately NOT run: both guard PDAs are absent.
    let (guard_token, _) =
        derive_guard_token_address(&owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let nonce = [71u8; 32];
    let (receipt, _) = derive_receipt_address(
        &owner.pubkey(),
        &fx.mint.pubkey(),
        &fx.source_owner.pubkey(),
        &nonce,
        &fx.program_id,
    );

    let held = |fx: &mut Fixture, amount: u64| {
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
                amount,
                6,
                nonce,
                HeldLimits::unlimited(),
                Some(PolicyTransferAccounts {
                    guard_token,
                    guard_state,
                    receipt,
                    bond_payer: fx.payer.pubkey(),
                }),
            )],
        )
    };

    held(&mut fx, 99).expect_err("held with no guard state must fail");
    fx.svm.expire_blockhash();
    // A credited transfer does not touch guard state, so it must still succeed.
    held(&mut fx, 150).expect("credited path must not require an initialized guard");
    assert_eq!(token_amount(&fx.svm, &dest.pubkey()), 150);
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
                HeldLimits::unlimited(),
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
    // A zero-amount hold moves nothing but still opens a receipt, so without this an
    // attacker holding no tokens at all could pile receipts onto a victim's shard.
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
            HeldLimits::unlimited(),
            Some(PolicyTransferAccounts {
                guard_token,
                guard_state,
                receipt,
                bond_payer: fx.payer.pubkey(),
            }),
        )],
    )
    .expect_err("zero-amount hold must be rejected");
}

#[test]
fn guard_state_accounts_for_every_held_token() {
    // held_amount is what makes `guard.amount >= sum(open receipts)` assertable rather than
    // merely true by construction. Track it across a hold and a claim.
    use token_2022_receive::guard::GuardState;

    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let read_state = |fx: &Fixture, key: &Pubkey| -> GuardState {
        let acct = fx.svm.get_account(key).expect("guard state");
        *bytemuck::from_bytes::<GuardState>(&acct.data[..std::mem::size_of::<GuardState>()])
    };

    let a = hold_99(&mut fx, [101u8; 32]);
    let gs = read_state(&fx, &a.guard_state);
    assert_eq!(gs.open_receipts, 1);
    assert_eq!(gs.held_amount, 99);
    assert!(token_amount(&fx.svm, &a.guard_token) >= gs.held_amount);

    // A second hold from the same sender: no capacity ceiling, and both are accounted.
    let b = hold_99(&mut fx, [102u8; 32]);
    let gs = read_state(&fx, &b.guard_state);
    assert_eq!(gs.open_receipts, 2);
    assert_eq!(gs.held_amount, 198);
    assert_eq!(token_amount(&fx.svm, &b.guard_token), 198);

    let claim_dest = fx.create_token_account(&fx.source_owner.pubkey());
    fx.svm.expire_blockhash();
    send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.source_owner],
        vec![claim_receipt(
            &fx.program_id,
            &a.receipt,
            &a.guard_token,
            &a.guard_state,
            &claim_dest.pubkey(),
            &fx.mint.pubkey(),
            &fx.source_owner.pubkey(),
            &fx.payer.pubkey(),
        )],
    )
    .expect("claim");

    let gs = read_state(&fx, &a.guard_state);
    assert_eq!(gs.open_receipts, 1, "one receipt still open");
    assert_eq!(gs.held_amount, 99, "and it is still backed");
    assert_eq!(token_amount(&fx.svm, &a.guard_token), 99);
}
