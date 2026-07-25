//! Host-side unit smoke tests (policy / TLV / PDAs / pack-unpack).
//! Stateful transfer paths live in `tests/verify_*.rs`.

use solana_program::pubkey::Pubkey;
use token_2022_receive::constants::{ALLOWLIST_CAP, DEFAULT_RECEIPT_TTL_SLOTS, MAX_OPEN_RECEIPTS};
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
    assert_eq!(MAX_OPEN_RECEIPTS, 64);
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
fn guard_state_capacity() {
    let mut gs = GuardState::new(
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    for _ in 0..MAX_OPEN_RECEIPTS {
        gs.try_increment_open().unwrap();
    }
    assert!(gs.try_increment_open().is_err());
    gs.try_decrement_open().unwrap();
    gs.try_increment_open().unwrap();
}

#[test]
fn instruction_pack_unpack_transfer() {
    let nonce = [7u8; 32];
    let ix = ReceiveTokenInstruction::TransferChecked {
        amount: 99,
        decimals: 6,
        unique_nonce: nonce,
    };
    let packed = ix.pack();
    let unpacked = ReceiveTokenInstruction::unpack(&packed).unwrap();
    match unpacked {
        ReceiveTokenInstruction::TransferChecked {
            amount,
            decimals,
            unique_nonce,
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
