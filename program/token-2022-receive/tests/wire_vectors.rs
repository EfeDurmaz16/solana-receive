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

/// Account-layout vector for the destination policy.
///
/// The JS client decodes this same byte layout in `clients/js/src/index.test.ts` so a sender can
/// read a destination's terms before paying. Two independent readers of one on-chain layout: if
/// either drifts, one of the two suites fails instead of a client silently misreading a policy.
#[test]
fn receive_policy_account_layout_vector() {
    use solana_program::pubkey::Pubkey;
    use token_2022_receive::extension::receive_policy::{ReceivePolicy, SourceOwnerMode};
    use token_2022_receive::extension::tlv::{
        account_len_with_receive_policy, write_receive_policy_tlv,
    };

    let mut policy = ReceivePolicy {
        min_amount: 100,
        source_owner_mode: SourceOwnerMode::Allowlist as u8,
        recovery_authority_mode: 1,
        _padding: [0; 6],
        recovery_authority: Pubkey::new_from_array([0xab; 32]),
        receipt_bond_lamports: 7,
        receipt_ttl_slots: 1_512_000,
        allowlist_len: 1,
        _padding2: [0; 7],
        allowlist: [Pubkey::default(); 8],
    };
    policy.allowlist[0] = Pubkey::new_from_array([0x11; 32]);

    let mut data = vec![0u8; account_len_with_receive_policy()];
    write_receive_policy_tlv(&mut data, &policy).unwrap();
    assert_eq!(
        data.len(),
        498,
        "165 base + type byte + 4 TLV header + 328 policy"
    );

    // Everything a reader needs to locate: type byte, TLV header, then each field in order.
    assert_eq!(
        hex(&data[165..234]),
        format!(
            "{}{}{}{}{}{}{}{}{}{}",
            "02",               // ACCOUNT_TYPE_ACCOUNT at offset 165
            "1027",             // extension type 10_000, little endian
            "4801",             // declared length 328
            "6400000000000000", // min_amount
            "01",               // source_owner_mode = Allowlist
            "01",               // recovery_authority_mode = Receiver
            "000000000000",     // padding
            "ab".repeat(32),    // recovery_authority
            "0700000000000000", // receipt_bond_lamports
            "4012170000000000", // receipt_ttl_slots
        )
    );
    assert_eq!(data[234], 1, "allowlist_len");
    assert_eq!(&data[242..274], &[0x11u8; 32], "allowlist[0]");
}

/// Tags 0, 1 and 7, and in particular the one field where a client encoder can silently produce
/// a body the program rejects.
///
/// `InitializeMint2`'s `Option<Pubkey>` is encoded as a u8 prefix with **no payload** when
/// `None`. A fixed-size option would emit 32 zero bytes after the prefix, and since `unpack`
/// requires each arm to consume its input exactly, the program would reject that as trailing
/// bytes. Nothing else pins this shape, so a codegen change that flips it would otherwise land
/// green.
#[test]
fn remaining_tag_wire_vectors() {
    use solana_program::pubkey::Pubkey;

    let authority = Pubkey::new_from_array([0xcd; 32]);
    let freeze = Pubkey::new_from_array([0xef; 32]);

    let none = ReceiveTokenInstruction::InitializeMint2 {
        decimals: 6,
        mint_authority: authority,
        freeze_authority: None,
    }
    .pack();
    assert_eq!(none.len(), 35, "tag + decimals + authority + option prefix");
    assert_eq!(hex(&none), format!("00{}{}{}", "06", "cd".repeat(32), "00"));

    let some = ReceiveTokenInstruction::InitializeMint2 {
        decimals: 6,
        mint_authority: authority,
        freeze_authority: Some(freeze),
    }
    .pack();
    assert_eq!(some.len(), 67);
    assert_eq!(
        hex(&some),
        format!("00{}{}{}{}", "06", "cd".repeat(32), "01", "ef".repeat(32))
    );

    let init_account = ReceiveTokenInstruction::InitializeAccount3 {
        owner: Pubkey::new_from_array([0x11; 32]),
    }
    .pack();
    assert_eq!(init_account.len(), 33);
    assert_eq!(hex(&init_account), format!("01{}", "11".repeat(32)));

    let mint_to = ReceiveTokenInstruction::MintTo { amount: 7 }.pack();
    assert_eq!(mint_to.len(), 9);
    assert_eq!(hex(&mint_to), format!("07{}", "0700000000000000"));
}
