//! Cross-language wire contract.
//!
//! These byte vectors are asserted identically by the JS client in
//! `clients/js/src/index.test.ts`. The client and the program have independent encoders, so
//! without a shared vector a divergence shows up only as a runtime decode failure on chain.
//! Change one side and this suite (plus its JS twin) must change with it.

use token_2022_receive::instruction::{HeldLimits, ReceiveTokenInstruction};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn transfer_checked_wire_vector() {
    let packed = ReceiveTokenInstruction::TransferChecked {
        amount: 1,
        decimals: 6,
        unique_nonce: [9u8; 32],
        limits: HeldLimits::unlimited(),
    }
    .pack();
    assert_eq!(packed.len(), 59);
    assert_eq!(
        hex(&packed),
        format!(
            "04{}{}{}{}{}{}",
            "0100000000000000", // amount
            "06",               // decimals
            "09".repeat(32),    // unique_nonce
            "ffffffffffffffff", // max_bond_lamports
            "ffffffffffffffff", // max_ttl_slots
            "02",               // max_recovery_mode: ThirdParty, i.e. accept any
        )
    );
}

#[test]
fn initialize_receive_policy_wire_vector() {
    let packed = ReceiveTokenInstruction::InitializeReceivePolicy {
        min_amount: 100,
        source_owner_mode: 1,
        recovery_authority_mode: 2,
        recovery_authority: solana_program::pubkey::Pubkey::new_from_array([0xab; 32]),
        receipt_bond_lamports: 0,
        receipt_ttl_slots: 1_512_000,
        allowlist: vec![],
    }
    .pack();
    assert_eq!(
        hex(&packed),
        format!(
            "02{}{}{}{}{}{}",
            "6400000000000000", // min_amount
            "0102",             // source_owner_mode, recovery_authority_mode
            "ab".repeat(32),    // recovery_authority
            "0000000000000000", // receipt_bond_lamports
            "4012170000000000", // receipt_ttl_slots = 0x171240, little endian
            "00",               // allowlist_len
        )
    );
}

/// The one variable-length field: without this the shared contract only pins the empty case.
#[test]
fn initialize_receive_policy_allowlist_wire_vector() {
    let packed = ReceiveTokenInstruction::InitializeReceivePolicy {
        min_amount: 0,
        source_owner_mode: 1,
        recovery_authority_mode: 0,
        recovery_authority: solana_program::pubkey::Pubkey::new_from_array([0; 32]),
        receipt_bond_lamports: 0,
        receipt_ttl_slots: 0,
        allowlist: vec![
            solana_program::pubkey::Pubkey::new_from_array([0x11; 32]),
            solana_program::pubkey::Pubkey::new_from_array([0x22; 32]),
        ],
    }
    .pack();
    assert_eq!(
        hex(&packed),
        format!(
            "02{}{}{}{}{}{}{}{}",
            "0000000000000000", // min_amount
            "0100",             // modes
            "00".repeat(32),    // recovery_authority
            "0000000000000000", // bond
            "0000000000000000", // ttl
            "02",               // allowlist_len
            "11".repeat(32),
            "22".repeat(32),
        )
    );
}

#[test]
fn tag_only_instructions_are_single_bytes() {
    assert_eq!(ReceiveTokenInstruction::EnsureGuard.pack(), vec![3]);
    assert_eq!(ReceiveTokenInstruction::ClaimReceipt.pack(), vec![5]);
    assert_eq!(ReceiveTokenInstruction::CloseExpiredReceipt.pack(), vec![6]);
}
