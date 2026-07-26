//! Regression suite: every PDA this program creates must survive being pre-funded.
//!
//! All of these addresses are derivable by anyone from public inputs, and anyone may credit
//! lamports to any address. `system_instruction::create_account` fails outright when the
//! target already holds lamports, so plain create_account let one lamport of dust brick a
//! guard shard permanently (killing all held delivery for that receiver/mint) or block an
//! individual receipt.

#[path = "common/litesvm.rs"]
mod litesvm_helpers;

use litesvm_helpers::{send, token_amount, Fixture};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::system_instruction;
use token_2022_receive::guard::{derive_guard_state_address, derive_guard_token_address};
use token_2022_receive::instruction::HeldLimits;
use token_2022_receive::instruction::{ensure_guard, transfer_checked, PolicyTransferAccounts};
use token_2022_receive::receipt::derive_receipt_address;

fn dust(fx: &mut Fixture, target: &Pubkey, lamports: u64) {
    send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![system_instruction::transfer(
            &fx.payer.pubkey(),
            target,
            lamports,
        )],
    )
    .expect("dust target");
    fx.svm.expire_blockhash();
}

#[test]
fn guard_creation_survives_prefunded_guard_token() {
    let mut fx = Fixture::boot(1_000).with_plain_dest();
    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    dust(&mut fx, &guard_token, 1);

    send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![ensure_guard(
            &fx.program_id,
            &fx.payer.pubkey(),
            &fx.dest_owner.pubkey(),
            &fx.mint.pubkey(),
            &guard_token,
            &guard_state,
        )],
    )
    .expect("guard must be creatable despite dust");

    // And it is a usable token account, not a half-initialized husk.
    assert_eq!(token_amount(&fx.svm, &guard_token), 0);
}

#[test]
fn guard_creation_survives_prefunded_guard_state() {
    let mut fx = Fixture::boot(1_000).with_plain_dest();
    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    dust(&mut fx, &guard_state, 1);

    send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![ensure_guard(
            &fx.program_id,
            &fx.payer.pubkey(),
            &fx.dest_owner.pubkey(),
            &fx.mint.pubkey(),
            &guard_token,
            &guard_state,
        )],
    )
    .expect("guard state must be creatable despite dust");
}

#[test]
fn held_transfer_survives_a_prefunded_receipt_pda() {
    let mut fx = Fixture::boot(1_000).with_policy_dest(100);
    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let nonce = [41u8; 32];
    let (receipt, _) = derive_receipt_address(
        &fx.dest_owner.pubkey(),
        &fx.mint.pubkey(),
        &fx.source_owner.pubkey(),
        &nonce,
        &fx.program_id,
    );
    // Anyone who can predict (receiver, mint, source_owner, nonce) could otherwise block
    // this exact held transfer.
    dust(&mut fx, &receipt, 1);

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
    .expect("held transfer must survive dust on the receipt PDA");

    assert_eq!(token_amount(&fx.svm, &guard_token), 99);
    let acct = fx.svm.get_account(&receipt).expect("receipt");
    assert!(
        acct.lamports > 1,
        "receipt funded to at least rent exemption"
    );
    assert_eq!(acct.owner, fx.program_id, "receipt owned by the program");
}
