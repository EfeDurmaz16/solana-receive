//! Host-side unit smoke tests (policy / TLV / PDAs / pack-unpack).
//! Stateful transfer paths live in `tests/verify_*.rs`.

use solana_program::pubkey::Pubkey;
use token_2022_receive::constants::{ALLOWLIST_CAP, DEFAULT_RECEIPT_TTL_SLOTS};
use token_2022_receive::extension::receive_policy::{ReceivePolicy, SourceOwnerMode};
use token_2022_receive::extension::tlv::{
    account_len_with_receive_policy, get_receive_policy, has_receive_policy,
    write_receive_policy_tlv,
};
use token_2022_receive::guard::{
    derive_guard_state_address, derive_guard_token_address, GuardState,
};
use token_2022_receive::instruction::ReceiveTokenInstruction;
use token_2022_receive::receipt::derive_receipt_address;
use token_2022_receive::state::{ACCOUNT_SIZE, MINT_SIZE};

#[test]
fn sizes_match_token_layout() {
    assert_eq!(MINT_SIZE, 82);
    assert_eq!(ACCOUNT_SIZE, 165);
    assert_eq!(DEFAULT_RECEIPT_TTL_SLOTS, 1_512_000);
    assert_eq!(ALLOWLIST_CAP, 8);
}

#[test]
fn policy_allow_all_credits_above_min() {
    let mut policy = ReceivePolicy::default();
    policy.min_amount = 100;
    policy.source_owner_mode = SourceOwnerMode::AllowAll as u8;
    let owner = Pubkey::new_unique();
    assert!(policy.accepts(100, &owner).unwrap());
    assert!(policy.accepts(101, &owner).unwrap());
    assert!(!policy.accepts(99, &owner).unwrap());
}

#[test]
fn policy_allowlist_membership_uses_source_owner() {
    let allowed = Pubkey::new_unique();
    let other = Pubkey::new_unique();
    let mut policy = ReceivePolicy::default();
    policy.source_owner_mode = SourceOwnerMode::Allowlist as u8;
    policy.allowlist_len = 1;
    policy.allowlist[0] = allowed;
    assert!(policy.accepts(1, &allowed).unwrap());
    assert!(!policy.accepts(1, &other).unwrap());
}

#[test]
fn tlv_roundtrip_receive_policy() {
    let mut data = vec![0u8; account_len_with_receive_policy()];
    // Base must look initialized enough for type byte path — write_receive_policy_tlv sets AccountType.
    let mut policy = ReceivePolicy::default();
    policy.min_amount = 42;
    policy.source_owner_mode = SourceOwnerMode::Allowlist as u8;
    policy.allowlist_len = 2;
    policy.allowlist[0] = Pubkey::new_unique();
    policy.allowlist[1] = Pubkey::new_unique();
    write_receive_policy_tlv(&mut data, &policy).unwrap();
    assert!(has_receive_policy(&data).unwrap());
    let got = get_receive_policy(&data).unwrap();
    assert_eq!(got.min_amount, 42);
    assert_eq!(got.allowlist_len, 2);
    assert_eq!(got.allowlist[0], policy.allowlist[0]);
}

#[test]
fn guard_shard_pdas_differ_by_receiver() {
    let program_id = token_2022_receive::id();
    let mint = Pubkey::new_unique();
    let r1 = Pubkey::new_unique();
    let r2 = Pubkey::new_unique();
    let (g1, _) = derive_guard_token_address(&r1, &mint, &program_id);
    let (g2, _) = derive_guard_token_address(&r2, &mint, &program_id);
    assert_ne!(g1, g2);
    let (s1, _) = derive_guard_state_address(&r1, &mint, &program_id);
    let (s2, _) = derive_guard_state_address(&r2, &mint, &program_id);
    assert_ne!(s1, s2);
}

#[test]
fn receipt_pda_includes_nonce_no_global_writable() {
    let program_id = token_2022_receive::id();
    let receiver = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let source_owner = Pubkey::new_unique();
    let mut n1 = [0u8; 32];
    n1[0] = 1;
    let mut n2 = [0u8; 32];
    n2[0] = 2;
    let (a, _) = derive_receipt_address(&receiver, &mint, &source_owner, &n1, &program_id);
    let (b, _) = derive_receipt_address(&receiver, &mint, &source_owner, &n2, &program_id);
    assert_ne!(a, b);
}

#[test]
fn guard_state_tracks_held_amount_not_a_capacity() {
    // The old per-shard receipt cap was removed: it was a shared permissionless resource that
    // anyone could exhaust to deny every other sender held delivery, while protecting nobody
    // (the bond payer funds each receipt's rent, never the receiver). held_amount replaces it
    // with an invariant that can actually be asserted.
    let mut gs = GuardState::new(
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    for i in 0..1_000u64 {
        gs.record_hold(10).unwrap();
        assert_eq!(gs.open_receipts, i + 1);
    }
    assert_eq!(gs.held_amount, 10_000);

    gs.record_release(10).unwrap();
    assert_eq!(gs.open_receipts, 999);
    assert_eq!(gs.held_amount, 9_990);

    // Releasing more than is held, or more receipts than are open, must not wrap.
    assert!(gs.record_release(u64::MAX).is_err());
    let mut empty = GuardState::new(
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    assert!(empty.record_release(0).is_err());
    assert!(GuardState::new(
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique()
    )
    .record_hold(u64::MAX)
    .is_ok());
}

#[test]
fn instruction_pack_unpack_transfer() {
    let nonce = [7u8; 32];
    let ix = ReceiveTokenInstruction::TransferChecked {
        amount: 99,
        decimals: 6,
        unique_nonce: nonce,
        limits: token_2022_receive::instruction::HeldLimits::unlimited(),
    };
    let packed = ix.pack();
    let unpacked = ReceiveTokenInstruction::unpack(&packed).unwrap();
    match unpacked {
        ReceiveTokenInstruction::TransferChecked {
            amount,
            decimals,
            unique_nonce,
            ..
        } => {
            assert_eq!(amount, 99);
            assert_eq!(decimals, 6);
            assert_eq!(unique_nonce, nonce);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn instruction_pack_unpack_policy_init() {
    let recovery = Pubkey::new_unique();
    let a = Pubkey::new_unique();
    let ix = ReceiveTokenInstruction::InitializeReceivePolicy {
        min_amount: 10,
        source_owner_mode: 1,
        recovery_authority_mode: 0,
        recovery_authority: recovery,
        receipt_bond_lamports: 1_000_000,
        receipt_ttl_slots: DEFAULT_RECEIPT_TTL_SLOTS,
        allowlist: vec![a],
    };
    let packed = ix.pack();
    let unpacked = ReceiveTokenInstruction::unpack(&packed).unwrap();
    match unpacked {
        ReceiveTokenInstruction::InitializeReceivePolicy {
            min_amount,
            allowlist,
            receipt_ttl_slots,
            ..
        } => {
            assert_eq!(min_amount, 10);
            assert_eq!(allowlist, vec![a]);
            assert_eq!(receipt_ttl_slots, DEFAULT_RECEIPT_TTL_SLOTS);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn plain_token_account_reports_no_policy() {
    // A bare 165-byte account carries no TLV region at all: absence, not corruption.
    let data = vec![0u8; token_2022_receive::state::ACCOUNT_SIZE];
    assert!(!has_receive_policy(&data).unwrap());
}

#[test]
fn malformed_policy_tlv_errors_instead_of_reporting_no_policy() {
    // Reporting `false` here would route the transfer down the no-policy path and credit the
    // destination, silently bypassing the policy the receiver attached.
    let mut data = vec![0u8; account_len_with_receive_policy()];
    write_receive_policy_tlv(&mut data, &ReceivePolicy::default()).unwrap();

    // Declared length shorter than ReceivePolicy: the reader must not run past it.
    let tlv_len_at = token_2022_receive::state::ACCOUNT_SIZE + 1 + 2;
    data[tlv_len_at..tlv_len_at + 2].copy_from_slice(&4u16.to_le_bytes());
    assert!(has_receive_policy(&data).is_err());
    assert!(get_receive_policy(&data).is_err());

    // Declared length overrunning the account.
    data[tlv_len_at..tlv_len_at + 2].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(has_receive_policy(&data).is_err());
}

#[test]
fn unpack_rejects_non_canonical_instruction_encodings() {
    use token_2022_receive::instruction::ReceiveTokenInstruction;

    // Round-trip: every packed form must decode back.
    for ix in [
        ReceiveTokenInstruction::EnsureGuard,
        ReceiveTokenInstruction::ClaimReceipt,
        ReceiveTokenInstruction::CloseExpiredReceipt,
        ReceiveTokenInstruction::MintTo { amount: 7 },
        ReceiveTokenInstruction::TransferChecked {
            amount: 1,
            decimals: 6,
            unique_nonce: [9u8; 32],
            limits: token_2022_receive::instruction::HeldLimits::unlimited(),
        },
    ] {
        let packed = ix.pack();
        assert_eq!(ReceiveTokenInstruction::unpack(&packed).unwrap(), ix);

        // Trailing bytes must not be silently ignored: otherwise there is no canonical wire
        // form and a mis-encoded instruction is reinterpreted rather than rejected.
        let mut extra = packed.clone();
        extra.push(0);
        assert!(
            ReceiveTokenInstruction::unpack(&extra).is_err(),
            "trailing byte accepted for {ix:?}"
        );
    }

    // The arms with real parsing: variable-length allowlist, optional freeze authority.
    for ix in [
        ReceiveTokenInstruction::InitializeAccount3 {
            owner: Pubkey::new_unique(),
        },
        ReceiveTokenInstruction::InitializeMint2 {
            decimals: 6,
            mint_authority: Pubkey::new_unique(),
            freeze_authority: Some(Pubkey::new_unique()),
        },
        ReceiveTokenInstruction::InitializeReceivePolicy {
            min_amount: 1,
            source_owner_mode: 1,
            recovery_authority_mode: 2,
            recovery_authority: Pubkey::new_unique(),
            receipt_bond_lamports: 2,
            receipt_ttl_slots: 3,
            allowlist: vec![Pubkey::new_unique(), Pubkey::new_unique()],
        },
    ] {
        let packed = ix.pack();
        assert_eq!(ReceiveTokenInstruction::unpack(&packed).unwrap(), ix);
        let mut extra = packed.clone();
        extra.push(0);
        assert!(
            ReceiveTokenInstruction::unpack(&extra).is_err(),
            "trailing byte accepted for {ix:?}"
        );
        // Truncation must not decode either.
        assert!(ReceiveTokenInstruction::unpack(&packed[..packed.len() - 1]).is_err());
    }

    // A freeze-authority flag pack can never emit must be rejected.
    let mut mint_ix = ReceiveTokenInstruction::InitializeMint2 {
        decimals: 6,
        mint_authority: Pubkey::new_unique(),
        freeze_authority: None,
    }
    .pack();
    *mint_ix.last_mut().unwrap() = 2;
    assert!(ReceiveTokenInstruction::unpack(&mint_ix).is_err());

    assert!(ReceiveTokenInstruction::unpack(&[]).is_err());
    assert!(ReceiveTokenInstruction::unpack(&[99]).is_err());
}

#[test]
fn foreign_extensions_are_rejected_alongside_a_receive_policy() {
    // SPEC section 9 says ReceivePolicy does not coexist with other account extensions in v0.
    // Before assert_no_other_extensions that was documentation only: the TLV walker skipped
    // past anything it did not recognise.
    use token_2022_receive::extension::tlv::assert_no_other_extensions;

    let mut data = vec![0u8; account_len_with_receive_policy() + 8];
    write_receive_policy_tlv(&mut data, &ReceivePolicy::default()).unwrap();
    assert!(
        assert_no_other_extensions(&data).is_ok(),
        "policy alone is fine"
    );

    // Append a second, unknown extension after the policy entry.
    let tail = account_len_with_receive_policy();
    data[tail..tail + 2].copy_from_slice(&7u16.to_le_bytes()); // some other extension type
    data[tail + 2..tail + 4].copy_from_slice(&0u16.to_le_bytes());
    assert!(
        assert_no_other_extensions(&data).is_err(),
        "a trailing foreign extension must be rejected"
    );

    // A plain account carries no TLV region at all.
    assert!(assert_no_other_extensions(&[0u8; token_2022_receive::state::ACCOUNT_SIZE]).is_ok());
}

#[test]
fn error_discriminants_are_stable() {
    // These surface to clients as ProgramError::Custom(n) and are quoted in docs, so they must
    // not move when a variant is retired. Retired slots 0, 5, 11, 12 and 16 stay empty.
    use token_2022_receive::error::ReceiveTokenError as E;
    for (variant, code) in [
        (E::InsufficientFunds, 1u32),
        (E::MintMismatch, 2),
        (E::AccountFrozen, 3),
        (E::OwnerMismatch, 4),
        (E::AlreadyInUse, 6),
        (E::InvalidInstruction, 7),
        (E::InvalidAccountData, 8),
        (E::MintDecimalsMismatch, 9),
        (E::MissingPolicyAccounts, 10),
        (E::PolicyNotEnabled, 13),
        (E::InvalidReceipt, 14),
        (E::ReceiptNotExpired, 15),
        (E::UnauthorizedClaim, 17),
        (E::AllowlistTooLarge, 18),
        (E::Overflow, 19),
        (E::InvalidPda, 20),
        (E::InvalidBondDestination, 21),
        (E::UnsupportedExtension, 22),
        (E::GuardNotTransferable, 23),
        (E::SelfTransferForbidden, 24),
        (E::InvalidPolicyMode, 25),
        (E::PolicyBondTooLarge, 26),
        (E::PolicyTtlTooLarge, 27),
        (E::GuardUnderfunded, 28),
        (E::BondAboveSenderLimit, 29),
        (E::TtlAboveSenderLimit, 30),
        (E::RecoveryModeAboveSenderLimit, 31),
        (E::UnsupportedStateVersion, 32),
    ] {
        assert_eq!(variant.clone() as u32, code, "{variant:?} moved");
    }
    assert_eq!(
        solana_program::program_error::ProgramError::from(E::SelfTransferForbidden),
        solana_program::program_error::ProgramError::Custom(24)
    );
    assert_eq!(
        E::SelfTransferForbidden.to_string(),
        "Source and destination must differ"
    );
}

#[test]
fn a_mint_cannot_be_allocated_at_token_account_size() {
    // Neither type carries a tag, so a mint with >= ACCOUNT_SIZE bytes would parse as an
    // uninitialized token account and InitializeAccount3 would overwrite it, bricking every
    // token account of that mint. Keeping the two disjoint by size is what prevents it.
    assert!(token_2022_receive::state::MINT_SIZE < token_2022_receive::state::ACCOUNT_SIZE);
}

#[test]
fn state_layouts_are_versioned() {
    // Without a version field a future layout change is only detectable as a length error, and a
    // same-size change is not detectable at all: live accounts would be silently reinterpreted.
    use token_2022_receive::guard::{GuardState, GUARD_STATE_VERSION};
    use token_2022_receive::receipt::{Receipt, RECEIPT_SIZE, RECEIPT_VERSION};

    let gs = GuardState::new(
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    assert_eq!(gs.version, GUARD_STATE_VERSION);
    assert_eq!(gs.held_amount, 0);

    // Receipt takes its version byte from existing padding, so the account size is unchanged.
    assert_eq!(RECEIPT_SIZE, 304);
    let r = Receipt {
        version: RECEIPT_VERSION,
        ..bytemuck::Zeroable::zeroed()
    };
    assert_eq!(r.version, 1);
}

#[test]
fn a_foreign_extension_is_rejected_even_without_a_receive_policy() {
    // SPEC section 9 says ReceivePolicy does not coexist with other account extensions in v0.
    // The check used to run only when a policy was present, so a destination carrying a foreign
    // extension and NO policy took the ordinary credit path: exactly the case the claim exists
    // to cover, since that account's semantics are undefined here.
    use token_2022_receive::extension::tlv::assert_no_other_extensions;

    let base = token_2022_receive::state::ACCOUNT_SIZE;
    let mut data = vec![0u8; base + 1 + 4 + 16];
    data[base] = 2; // ACCOUNT_TYPE_ACCOUNT
    data[base + 1..base + 3].copy_from_slice(&7u16.to_le_bytes()); // some other extension
    data[base + 3..base + 5].copy_from_slice(&16u16.to_le_bytes());
    assert!(
        assert_no_other_extensions(&data).is_err(),
        "a foreign extension must be rejected on its own, not only alongside a policy"
    );

    // And a policy on its own is still fine.
    let mut ok = vec![0u8; account_len_with_receive_policy()];
    write_receive_policy_tlv(&mut ok, &ReceivePolicy::default()).unwrap();
    assert!(assert_no_other_extensions(&ok).is_ok());
}
