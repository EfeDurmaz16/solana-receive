#[path = "common/litesvm.rs"]
mod litesvm_helpers;

use litesvm_helpers::{send, token_amount, Fixture};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signer;
use solana_sdk::system_instruction;
use token_2022_receive::constants::DEFAULT_RECEIPT_TTL_SLOTS;
use token_2022_receive::extension::receive_policy::SourceOwnerMode;
use token_2022_receive::extension::tlv::account_len_with_receive_policy;
use token_2022_receive::guard::{derive_guard_state_address, derive_guard_token_address};
use token_2022_receive::instruction::{
    initialize_account3, initialize_receive_policy, transfer_checked, PolicyTransferAccounts,
};
use token_2022_receive::receipt::derive_receipt_address;

#[test]
fn credited_transfer_without_ensure_guard() {
    let mut fx = Fixture::boot(1_000);
    let space = account_len_with_receive_policy();
    let rent_acc = fx.svm.minimum_balance_for_rent_exemption(space);
    send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.dest],
        vec![system_instruction::create_account(
            &fx.payer.pubkey(),
            &fx.dest.pubkey(),
            rent_acc,
            space as u64,
            &fx.program_id,
        )],
    )
    .expect("create policy dest");
    send(
        &mut fx.svm,
        &fx.payer,
        &[],
        vec![initialize_account3(
            &fx.program_id,
            &fx.dest.pubkey(),
            &fx.mint.pubkey(),
            &fx.dest_owner.pubkey(),
        )],
    )
    .expect("init policy dest");
    send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.dest_owner],
        vec![initialize_receive_policy(
            &fx.program_id,
            &fx.dest.pubkey(),
            &fx.dest_owner.pubkey(),
            10, // min_amount
            SourceOwnerMode::AllowAll as u8,
            0,
            Pubkey::default(),
            1_000_000,
            DEFAULT_RECEIPT_TTL_SLOTS,
            vec![],
        )],
    )
    .expect("init receive policy");

    // NOTE: no ensure_guard here.
    let (guard_token, _) =
        derive_guard_token_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let (guard_state, _) =
        derive_guard_state_address(&fx.dest_owner.pubkey(), &fx.mint.pubkey(), &fx.program_id);
    let nonce = [7u8; 32];
    let (receipt, _) = derive_receipt_address(
        &fx.dest_owner.pubkey(),
        &fx.mint.pubkey(),
        &fx.source_owner.pubkey(),
        &nonce,
        &fx.program_id,
    );

    let res = send(
        &mut fx.svm,
        &fx.payer,
        &[&fx.source_owner],
        vec![transfer_checked(
            &fx.program_id,
            &fx.source.pubkey(),
            &fx.mint.pubkey(),
            &fx.dest.pubkey(),
            &fx.source_owner.pubkey(),
            500, // >= min_amount 10 => policy ACCEPTS => credited path
            6,
            nonce,
            Some(PolicyTransferAccounts {
                guard_token,
                guard_state,
                receipt,
                bond_payer: fx.payer.pubkey(),
            }),
        )],
    );
    match res {
        Ok(meta) => {
            println!("PROBE: credited transfer SUCCEEDED without ensure_guard");
            println!("PROBE: return_data = {:?}", meta.return_data);
            println!("PROBE: logs = {:?}", meta.logs);
            assert_eq!(token_amount(&fx.svm, &fx.dest.pubkey()), 500);
        }
        Err(e) => panic!("PROBE: credited transfer FAILED without ensure_guard: {e:?}"),
    }
}
