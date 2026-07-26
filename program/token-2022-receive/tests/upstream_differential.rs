//! Honest upstream differential for the **overlapping no-policy surface**.
//!
//! This suite does **not** execute `TokenzQd…`. It pins layout sizes / Pack offsets
//! and no-policy amount semantics against the documented SPL Token / Token-2022 base
//! account format. Instruction tags in this reference program are **local** and are
//! intentionally not wire-compatible with upstream Token-2022 discriminators.
//!
//! ## Declared overlap
//! - Base mint size 82 / account size 165
//! - Pack field order for mint + token account (amount @ offset 64)
//! - No-policy TransferChecked amount debit/credit behavior
//!
//! ## Explicitly unsupported / out of scope here
//! - Full Token-2022 extension set (transfer fee, confidential, transfer hook, …)
//! - Wire-compatible instruction discriminators vs `TokenzQd`
//! - Legacy Tokenkeg USDC/USDT interception
//! - Ambient ATA policy on unmodified Token-2022

#[path = "common/host/mod.rs"]
mod host;

use host::no_policy_transfer;
use solana_program::{program_option::COption, program_pack::Pack, pubkey::Pubkey};
use token_2022_receive::instruction::ReceiveTokenInstruction;
use token_2022_receive::state::{AccountState, Mint, TokenAccount, ACCOUNT_SIZE, MINT_SIZE};

/// Pinned upstream Token / Token-2022 base layout sizes (spl_token Pack).
const UPSTREAM_MINT_LEN: usize = 82;
const UPSTREAM_ACCOUNT_LEN: usize = 165;
/// Token account `amount` field offset in the packed base layout.
const UPSTREAM_AMOUNT_OFFSET: usize = 64;

#[test]
fn overlap_sizes_match_upstream_base_layout() {
    assert_eq!(MINT_SIZE, UPSTREAM_MINT_LEN);
    assert_eq!(ACCOUNT_SIZE, UPSTREAM_ACCOUNT_LEN);
    assert_eq!(Mint::LEN, UPSTREAM_MINT_LEN);
    assert_eq!(TokenAccount::LEN, UPSTREAM_ACCOUNT_LEN);
}

#[test]
fn overlap_amount_pack_offset_matches_upstream() {
    let mint = Pubkey::new_from_array([1u8; 32]);
    let owner = Pubkey::new_from_array([2u8; 32]);
    let mut data = vec![0u8; ACCOUNT_SIZE];
    TokenAccount {
        mint,
        owner,
        amount: 0x0102_0304_0506_0708,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    }
    .pack_into_slice(&mut data);

    let encoded = u64::from_le_bytes(
        data[UPSTREAM_AMOUNT_OFFSET..UPSTREAM_AMOUNT_OFFSET + 8]
            .try_into()
            .unwrap(),
    );
    assert_eq!(encoded, 0x0102_0304_0506_0708);

    // mint (32) + owner (32) precede amount.
    assert_eq!(&data[0..32], mint.as_ref());
    assert_eq!(&data[32..64], owner.as_ref());
}

#[test]
fn overlap_mint_pack_roundtrip() {
    let authority = Pubkey::new_from_array([9u8; 32]);
    let mut data = vec![0u8; MINT_SIZE];
    Mint {
        mint_authority: COption::Some(authority),
        supply: 1_000_000,
        decimals: 6,
        is_initialized: true,
        freeze_authority: COption::None,
    }
    .pack_into_slice(&mut data);
    let got = Mint::unpack_from_slice(&data).unwrap();
    assert_eq!(got.decimals, 6);
    assert_eq!(got.supply, 1_000_000);
    assert_eq!(got.mint_authority, COption::Some(authority));
    assert!(got.is_initialized);
}

#[test]
fn overlap_no_policy_transfer_amount_deltas() {
    // Same debit/credit semantics as ordinary Token TransferChecked.
    let (r, s, d) = no_policy_transfer(1_000, 25, 75, 6);
    r.unwrap();
    assert_eq!(s, 925);
    assert_eq!(d, 100);
}

#[test]
fn reference_instruction_tags_are_local_not_tokenzqd_wire() {
    // Upstream Token-2022 (spl_token_interface): MintTo=7, TransferChecked=12,
    // InitializeAccount3=18, InitializeMint2=20.
    // This reference remaps a minimal subset to compact tags 0..=7.
    let mint2 = ReceiveTokenInstruction::InitializeMint2 {
        decimals: 6,
        mint_authority: Pubkey::default(),
        freeze_authority: None,
    }
    .pack();
    let acc3 = ReceiveTokenInstruction::InitializeAccount3 {
        owner: Pubkey::default(),
    }
    .pack();
    let xfer = ReceiveTokenInstruction::TransferChecked {
        amount: 1,
        decimals: 6,
        unique_nonce: [0u8; 32],
    }
    .pack();
    let mint_to = ReceiveTokenInstruction::MintTo { amount: 1 }.pack();

    assert_eq!(mint2[0], 0); // not upstream 20
    assert_eq!(acc3[0], 1); // not upstream 18
    assert_eq!(xfer[0], 4); // not upstream 12
    assert_eq!(mint_to[0], 7); // coincidentally same as upstream MintTo tag
    assert_eq!(xfer.len(), 42); // tag + amount + decimals + unique_nonce
}
