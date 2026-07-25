//! Compact host fixtures built on stubs.

use super::stubs::*;
use bytemuck::{bytes_of, from_bytes};
use solana_program::{entrypoint::ProgramResult, pubkey::Pubkey, rent::Rent, system_program};
use token_2022_receive::extension::receive_policy::ReceivePolicy;
use token_2022_receive::guard::{
    derive_guard_state_address, derive_guard_token_address, GuardState, GUARD_STATE_SIZE,
};
use token_2022_receive::instruction::{
    claim_receipt, close_expired_receipt, transfer_checked, PolicyTransferAccounts,
};
use token_2022_receive::process_instruction;
use token_2022_receive::receipt::{
    derive_receipt_address, Receipt, RECEIPT_DISCRIMINATOR, RECEIPT_SIZE,
};

/// Run a 4-account no-policy TransferChecked; returns (source_amt, dest_amt) after.
pub fn no_policy_transfer(
    source_amt: u64,
    dest_amt: u64,
    amount: u64,
    ix_decimals: u8,
) -> (ProgramResult, u64, u64) {
    let pid = program_id();
    let sys = system_pid();
    let mint_auth = Pubkey::new_unique();
    let mint = Pubkey::new_unique();
    let source_owner = Pubkey::new_unique();
    let dest_owner = Pubkey::new_unique();
    let source = Pubkey::new_unique();
    let dest = Pubkey::new_unique();

    let mut mint_lamports = 1;
    let mut source_lamports = 1;
    let mut dest_lamports = 1;
    let mut auth_lamports = 1;
    let mut mint_data = pack_mint(6, mint_auth);
    let mut source_data = pack_token(mint, source_owner, source_amt);
    let mut dest_data = pack_token(mint, dest_owner, dest_amt);
    let mut auth_data = vec![];

    let accounts = [
        ai(
            &source,
            false,
            true,
            &mut source_lamports,
            &mut source_data,
            &pid,
        ),
        ai(
            &mint,
            false,
            false,
            &mut mint_lamports,
            &mut mint_data,
            &pid,
        ),
        ai(&dest, false, true, &mut dest_lamports, &mut dest_data, &pid),
        ai(
            &source_owner,
            true,
            false,
            &mut auth_lamports,
            &mut auth_data,
            sys,
        ),
    ];
    let ix = transfer_checked(
        &pid,
        &source,
        &mint,
        &dest,
        &source_owner,
        amount,
        ix_decimals,
        [0u8; 32],
        None,
    );
    let result = process_instruction(&pid, &accounts, &ix.data);
    (result, amount_of(&source_data), amount_of(&dest_data))
}

pub struct PolicyTransferResult {
    pub result: ProgramResult,
    pub source_amt: u64,
    pub dest_amt: u64,
    pub guard_amt: u64,
    pub receipt_lamports: u64,
    pub receipt_data: Vec<u8>,
    pub open_receipts: u8,
    pub receipt_owner: Pubkey,
    pub source_owner: Pubkey,
}

/// Policy-enabled TransferChecked with full metas (or omit metas when `with_metas` is false).
pub fn policy_transfer(
    policy: &ReceivePolicy,
    source_amt: u64,
    amount: u64,
    nonce: [u8; 32],
    with_metas: bool,
) -> PolicyTransferResult {
    policy_transfer_ex(
        policy,
        source_amt,
        amount,
        nonce,
        PolicyTransferOpts {
            with_metas,
            ..PolicyTransferOpts::default()
        },
    )
}

#[derive(Clone, Copy)]
pub struct PolicyTransferOpts {
    pub with_metas: bool,
    /// Seed `guard_state.open_receipts` before the transfer (capacity tests).
    pub open_receipts: u8,
    /// If true, prefill receipt account data so a held create hits `AlreadyInUse`.
    pub receipt_prefilled: bool,
    /// Fixed source owner (allowlist membership tests).
    pub source_owner: Option<Pubkey>,
}

impl Default for PolicyTransferOpts {
    fn default() -> Self {
        Self {
            with_metas: true,
            open_receipts: 0,
            receipt_prefilled: false,
            source_owner: None,
        }
    }
}

pub fn policy_transfer_ex(
    policy: &ReceivePolicy,
    source_amt: u64,
    amount: u64,
    nonce: [u8; 32],
    opts: PolicyTransferOpts,
) -> PolicyTransferResult {
    with_stubs(|| {
        let pid = program_id();
        let sys = *system_pid();
        let receipt_owner = if opts.receipt_prefilled {
            pid
        } else {
            system_program::id()
        };
        let mint_auth = Pubkey::new_unique();
        let mint = Pubkey::new_unique();
        let source_owner = opts.source_owner.unwrap_or_else(Pubkey::new_unique);
        let dest_owner = Pubkey::new_unique();
        let source = Pubkey::new_unique();
        let dest = Pubkey::new_unique();
        let bond_payer = Pubkey::new_unique();

        let (guard_token, _) = derive_guard_token_address(&dest_owner, &mint, &pid);
        let (guard_state, _) = derive_guard_state_address(&dest_owner, &mint, &pid);
        let (receipt, _) = derive_receipt_address(&dest_owner, &mint, &source_owner, &nonce, &pid);

        let mut mint_lamports = 1;
        let mut source_lamports = 1;
        let mut dest_lamports = 1;
        let mut auth_lamports = 1;
        let mut guard_token_lamports = 1;
        let mut guard_state_lamports = 1;
        let mut receipt_lamports = if opts.receipt_prefilled { 1 } else { 0 };
        let mut bond_lamports = 50_000_000;
        let mut sys_lamports = 1;

        let mut mint_data = pack_mint(6, mint_auth);
        let mut source_data = pack_token(mint, source_owner, source_amt);
        let mut dest_data = pack_policy_account(mint, dest_owner, 0, policy);
        let mut auth_data = vec![];
        let mut guard_token_data = pack_token(mint, pid, 0);
        let mut gs = GuardState::new(dest_owner, mint, guard_token);
        gs.open_receipts = opts.open_receipts;
        let mut guard_state_data = bytes_of(&gs).to_vec();
        let mut receipt_buf = if opts.receipt_prefilled {
            vec![0xABu8; RECEIPT_SIZE]
        } else {
            vec![0u8; RECEIPT_SIZE]
        };
        let mut bond_data = vec![];
        let mut sys_data = vec![];

        let result = if opts.with_metas {
            let receipt_data: &mut [u8] = if opts.receipt_prefilled {
                &mut receipt_buf[..]
            } else {
                empty_into(&mut receipt_buf)
            };
            let accounts = [
                ai(
                    &source,
                    false,
                    true,
                    &mut source_lamports,
                    &mut source_data,
                    &pid,
                ),
                ai(
                    &mint,
                    false,
                    false,
                    &mut mint_lamports,
                    &mut mint_data,
                    &pid,
                ),
                ai(&dest, false, true, &mut dest_lamports, &mut dest_data, &pid),
                ai(
                    &source_owner,
                    true,
                    false,
                    &mut auth_lamports,
                    &mut auth_data,
                    &sys,
                ),
                ai(
                    &guard_token,
                    false,
                    true,
                    &mut guard_token_lamports,
                    &mut guard_token_data,
                    &pid,
                ),
                ai(
                    &guard_state,
                    false,
                    true,
                    &mut guard_state_lamports,
                    &mut guard_state_data,
                    &pid,
                ),
                ai(
                    &receipt,
                    false,
                    true,
                    &mut receipt_lamports,
                    receipt_data,
                    &receipt_owner,
                ),
                ai(
                    &bond_payer,
                    true,
                    true,
                    &mut bond_lamports,
                    &mut bond_data,
                    &sys,
                ),
                ai(&sys, false, false, &mut sys_lamports, &mut sys_data, &sys),
            ];
            let ix = transfer_checked(
                &pid,
                &source,
                &mint,
                &dest,
                &source_owner,
                amount,
                6,
                nonce,
                Some(PolicyTransferAccounts {
                    guard_token,
                    guard_state,
                    receipt,
                    bond_payer,
                }),
            );
            process_instruction(&pid, &accounts, &ix.data)
        } else {
            let accounts = [
                ai(
                    &source,
                    false,
                    true,
                    &mut source_lamports,
                    &mut source_data,
                    &pid,
                ),
                ai(
                    &mint,
                    false,
                    false,
                    &mut mint_lamports,
                    &mut mint_data,
                    &pid,
                ),
                ai(&dest, false, true, &mut dest_lamports, &mut dest_data, &pid),
                ai(
                    &source_owner,
                    true,
                    false,
                    &mut auth_lamports,
                    &mut auth_data,
                    &sys,
                ),
            ];
            let ix = transfer_checked(
                &pid,
                &source,
                &mint,
                &dest,
                &source_owner,
                amount,
                6,
                nonce,
                None,
            );
            process_instruction(&pid, &accounts, &ix.data)
        };

        let open_receipts = if guard_state_data.len() >= GUARD_STATE_SIZE {
            from_bytes::<GuardState>(&guard_state_data[..GUARD_STATE_SIZE]).open_receipts
        } else {
            0
        };

        PolicyTransferResult {
            result,
            source_amt: amount_of(&source_data),
            dest_amt: amount_of(&dest_data),
            guard_amt: amount_of(&guard_token_data),
            receipt_lamports,
            receipt_data: receipt_buf,
            open_receipts,
            receipt_owner,
            source_owner,
        }
    })
}

pub fn assert_held_receipt(out: &PolicyTransferResult, amount: u64, expires_slot: u64) {
    let receipt = from_bytes::<Receipt>(&out.receipt_data[..RECEIPT_SIZE]);
    assert_eq!(receipt.discriminator, RECEIPT_DISCRIMINATOR);
    assert_eq!(receipt.amount, amount);
    assert_eq!(receipt.source_owner, out.source_owner);
    assert_eq!(receipt.expires_slot, expires_slot);
    assert_eq!(out.receipt_owner, program_id());
    assert_eq!(out.open_receipts, 1);
    assert!(out.receipt_lamports >= Rent::default().minimum_balance(RECEIPT_SIZE));
}

pub struct ReceiptLifecycleResult {
    pub result: ProgramResult,
    pub guard_amt: u64,
    pub dest_amt: u64,
    pub bond_lamports: u64,
    pub receipt_lamports: u64,
    pub open_receipts: u8,
}

#[derive(Clone, Copy, Default)]
pub struct ClaimCloseOpts {
    pub bond_dest_override: Option<Pubkey>,
    pub guard_token_override: Option<Pubkey>,
    pub guard_state_override: Option<Pubkey>,
    /// Claim path only: wrong signer vs recorded claim authority.
    pub claim_authority_override: Option<Pubkey>,
}

enum Settle {
    Claim,
    Close { dest_prior: u64, slot: u64 },
}

fn settle_receipt(
    amount: u64,
    bond: u64,
    created: u64,
    expires: u64,
    mode: Settle,
    opts: ClaimCloseOpts,
) -> ReceiptLifecycleResult {
    with_stubs(|| {
        if let Settle::Close { slot, .. } = mode {
            set_slot(slot);
        }
        use token_2022_receive::extension::receive_policy::RecoveryAuthorityMode;
        let pid = program_id();
        let sys = system_pid();
        let mint = Pubkey::new_unique();
        let source_owner = Pubkey::new_unique();
        let dest_owner = Pubkey::new_unique();
        let token_dest = Pubkey::new_unique();
        let bond_payer = Pubkey::new_unique();
        let bond_dest = opts.bond_dest_override.unwrap_or(bond_payer);
        let claim_authority = opts.claim_authority_override.unwrap_or(source_owner);
        let (guard_token_pda, _) = derive_guard_token_address(&dest_owner, &mint, &pid);
        let (guard_state_pda, _) = derive_guard_state_address(&dest_owner, &mint, &pid);
        let guard_token = opts.guard_token_override.unwrap_or(guard_token_pda);
        let guard_state = opts.guard_state_override.unwrap_or(guard_state_pda);
        let nonce = [1u8; 32];
        let (receipt_key, _) =
            derive_receipt_address(&dest_owner, &mint, &source_owner, &nonce, &pid);
        let recovery = match mode {
            Settle::Claim => RecoveryAuthorityMode::Originator,
            Settle::Close { .. } => RecoveryAuthorityMode::Receiver,
        };
        let dest_prior = match mode {
            Settle::Claim => 0,
            Settle::Close { dest_prior, .. } => dest_prior,
        };
        let receipt = Receipt::new(
            amount,
            mint,
            Pubkey::new_unique(),
            source_owner,
            Pubkey::new_unique(),
            dest_owner,
            recovery,
            Pubkey::default(),
            created,
            expires,
            bond,
            bond_payer,
            nonce,
        );

        let mut receipt_lamports = bond;
        let mut gtl = 1u64;
        let mut gsl = 1u64;
        let mut dest_l = 1u64;
        let mut mint_l = 1u64;
        let mut auth_l = 1u64;
        let mut bond_l = 0u64;
        let mut receipt_data = bytes_of(&receipt).to_vec();
        let mut guard_token_data = pack_token(mint, pid, amount);
        let mut gs = GuardState::new(dest_owner, mint, guard_token_pda);
        gs.open_receipts = 1;
        let mut guard_state_data = bytes_of(&gs).to_vec();
        let mut dest_data = pack_token(mint, source_owner, dest_prior);
        let mut mint_data = pack_mint(6, Pubkey::new_unique());
        let mut auth_data = vec![];
        let mut bond_data = vec![];

        let result = match mode {
            Settle::Claim => {
                let accounts = [
                    ai(
                        &receipt_key,
                        false,
                        true,
                        &mut receipt_lamports,
                        &mut receipt_data,
                        &pid,
                    ),
                    ai(
                        &guard_token,
                        false,
                        true,
                        &mut gtl,
                        &mut guard_token_data,
                        &pid,
                    ),
                    ai(
                        &guard_state,
                        false,
                        true,
                        &mut gsl,
                        &mut guard_state_data,
                        &pid,
                    ),
                    ai(&token_dest, false, true, &mut dest_l, &mut dest_data, &pid),
                    ai(&mint, false, false, &mut mint_l, &mut mint_data, &pid),
                    ai(
                        &claim_authority,
                        true,
                        false,
                        &mut auth_l,
                        &mut auth_data,
                        sys,
                    ),
                    ai(&bond_dest, false, true, &mut bond_l, &mut bond_data, sys),
                ];
                let ix = claim_receipt(
                    &pid,
                    &receipt_key,
                    &guard_token,
                    &guard_state,
                    &token_dest,
                    &mint,
                    &claim_authority,
                    &bond_dest,
                );
                process_instruction(&pid, &accounts, &ix.data)
            }
            Settle::Close { .. } => {
                let accounts = [
                    ai(
                        &receipt_key,
                        false,
                        true,
                        &mut receipt_lamports,
                        &mut receipt_data,
                        &pid,
                    ),
                    ai(
                        &guard_token,
                        false,
                        true,
                        &mut gtl,
                        &mut guard_token_data,
                        &pid,
                    ),
                    ai(
                        &guard_state,
                        false,
                        true,
                        &mut gsl,
                        &mut guard_state_data,
                        &pid,
                    ),
                    ai(&token_dest, false, true, &mut dest_l, &mut dest_data, &pid),
                    ai(&mint, false, false, &mut mint_l, &mut mint_data, &pid),
                    ai(&bond_dest, false, true, &mut bond_l, &mut bond_data, sys),
                ];
                let ix = close_expired_receipt(
                    &pid,
                    &receipt_key,
                    &guard_token,
                    &guard_state,
                    &token_dest,
                    &mint,
                    &bond_dest,
                );
                process_instruction(&pid, &accounts, &ix.data)
            }
        };
        ReceiptLifecycleResult {
            result,
            guard_amt: amount_of(&guard_token_data),
            dest_amt: amount_of(&dest_data),
            bond_lamports: bond_l,
            receipt_lamports,
            open_receipts: from_bytes::<GuardState>(&guard_state_data[..GUARD_STATE_SIZE])
                .open_receipts,
        }
    })
}

pub fn run_claim(amount: u64, bond: u64, created: u64, expires: u64) -> ReceiptLifecycleResult {
    settle_receipt(
        amount,
        bond,
        created,
        expires,
        Settle::Claim,
        ClaimCloseOpts::default(),
    )
}

pub fn run_claim_ex(
    amount: u64,
    bond: u64,
    created: u64,
    expires: u64,
    opts: ClaimCloseOpts,
) -> ReceiptLifecycleResult {
    settle_receipt(amount, bond, created, expires, Settle::Claim, opts)
}

pub fn run_close_expired(
    amount: u64,
    dest_prior: u64,
    bond: u64,
    created: u64,
    expires: u64,
    slot: u64,
) -> ReceiptLifecycleResult {
    settle_receipt(
        amount,
        bond,
        created,
        expires,
        Settle::Close { dest_prior, slot },
        ClaimCloseOpts::default(),
    )
}

pub fn run_close_expired_ex(
    amount: u64,
    dest_prior: u64,
    bond: u64,
    created: u64,
    expires: u64,
    slot: u64,
    opts: ClaimCloseOpts,
) -> ReceiptLifecycleResult {
    settle_receipt(
        amount,
        bond,
        created,
        expires,
        Settle::Close { dest_prior, slot },
        opts,
    )
}

pub fn ix_account_counts() -> (usize, usize, usize, usize) {
    let pid = program_id();
    let k: Vec<Pubkey> = (0..8).map(|_| Pubkey::new_unique()).collect();
    let policy = transfer_checked(
        &pid,
        &k[0],
        &k[1],
        &k[2],
        &k[3],
        1,
        6,
        [0u8; 32],
        Some(PolicyTransferAccounts {
            guard_token: k[4],
            guard_state: k[5],
            receipt: k[6],
            bond_payer: k[7],
        }),
    );
    let no_policy = transfer_checked(&pid, &k[0], &k[1], &k[2], &k[3], 1, 6, [0u8; 32], None);
    let c: Vec<Pubkey> = (0..7).map(|_| Pubkey::new_unique()).collect();
    let claim = claim_receipt(&pid, &c[0], &c[1], &c[2], &c[3], &c[4], &c[5], &c[6]);
    let close = close_expired_receipt(&pid, &c[0], &c[1], &c[2], &c[3], &c[4], &c[5]);
    (
        no_policy.accounts.len(),
        policy.accounts.len(),
        claim.accounts.len(),
        close.accounts.len(),
    )
}

pub fn policy_ix_data_len() -> usize {
    let pid = program_id();
    let k: Vec<Pubkey> = (0..8).map(|_| Pubkey::new_unique()).collect();
    transfer_checked(
        &pid,
        &k[0],
        &k[1],
        &k[2],
        &k[3],
        1,
        6,
        [0u8; 32],
        Some(PolicyTransferAccounts {
            guard_token: k[4],
            guard_state: k[5],
            receipt: k[6],
            bond_payer: k[7],
        }),
    )
    .data
    .len()
}
