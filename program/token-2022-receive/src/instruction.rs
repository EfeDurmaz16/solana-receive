//! Instruction definitions for the reference receive Token program.

use crate::constants::ALLOWLIST_CAP;
use crate::error::ReceiveTokenError;
use crate::extension::receive_policy::RecoveryAuthorityMode;
use solana_program::{
    instruction::{AccountMeta, Instruction},
    program_error::ProgramError,
    pubkey::Pubkey,
    system_program,
};
use std::convert::TryInto;

#[repr(u8)]
#[derive(Clone, Debug, PartialEq)]
pub enum ReceiveTokenInstruction {
    /// Initialize a mint (minimal Token-2022 InitializeMint2 shape).
    /// Accounts: mint (w), rent sysvar (ignored — rent checked by runtime)
    InitializeMint2 {
        decimals: u8,
        mint_authority: Pubkey,
        freeze_authority: Option<Pubkey>,
    } = 0,

    /// Initialize a token account.
    /// Accounts: account (w), mint, owner (as data), rent ignored
    InitializeAccount3 { owner: Pubkey } = 1,

    /// Initialize ReceivePolicy extension on an allocated destination account.
    /// Accounts: token_account (w), owner (signer)
    InitializeReceivePolicy {
        min_amount: u64,
        source_owner_mode: u8,
        recovery_authority_mode: u8,
        recovery_authority: Pubkey,
        receipt_bond_lamports: u64,
        receipt_ttl_slots: u64,
        allowlist: Vec<Pubkey>,
    } = 2,

    /// Ensure guard token account + guard state PDA exist for (receiver, mint).
    /// Accounts: payer (signer, w), receiver, mint, guard_token (w), guard_state (w),
    EnsureGuard = 3,

    /// TransferChecked with optional held delivery.
    ///
    /// No-policy destination (standard accounts):
    ///   source (w), mint, destination (w), authority (signer)
    ///
    /// Policy destination (held path may need extras — always require when policy present):
    ///   source (w), mint, destination (w), authority (signer),
    ///   guard_token (w), guard_state (w), receipt (w), bond_payer (signer, w),
    ///   system_program
    ///
    /// Clock and Rent are read via syscall, not passed as accounts.
    ///
    /// `unique_nonce` is client-supplied 32 bytes for receipt PDA uniqueness.
    ///
    /// `limits` are the sender's terms for a held outcome. The destination writes the policy,
    /// but the sender pays for it: the bond is debited from `bond_payer` and the TTL decides how
    /// long a rejected transfer stays locked. Without them a sender has no way to refuse a
    /// destination that quietly raised either. `HeldLimits::unlimited()` preserves the old
    /// behaviour, and `max_ttl_slots: 0` means "never hold me, fail instead".
    TransferChecked {
        amount: u64,
        decimals: u8,
        unique_nonce: [u8; 32],
        limits: HeldLimits,
    } = 4,

    /// Full-claim held receipt → destination.
    /// Accounts: receipt (w), guard_token (w), guard_state (w), claim_destination (w),
    ///           mint, claim_authority (signer), bond_dest (w)
    ClaimReceipt = 5,

    /// Permissionless close after TTL: return tokens to source_owner ATA, refund bond.
    /// Accounts: receipt (w), guard_token (w), guard_state (w), source_owner_ata (w),
    ///           mint, bond_dest (w)
    CloseExpiredReceipt = 6,

    /// MintTo (minimal).
    /// Accounts: mint (w), account (w), mint_authority (signer)
    MintTo { amount: u64 } = 7,
}

impl ReceiveTokenInstruction {
    /// Decode instruction data.
    ///
    /// Every arm must consume its input exactly. Tolerating trailing bytes means there is no
    /// canonical wire form, so a misrouted or mis-encoded instruction gets silently
    /// reinterpreted as a valid one instead of rejected.
    pub fn unpack(input: &[u8]) -> Result<Self, ProgramError> {
        let (&tag, rest) = input
            .split_first()
            .ok_or(ReceiveTokenError::InvalidInstruction)?;
        let mut trailing = rest;
        let parsed = Self::unpack_body(tag, rest, &mut trailing)?;
        if !trailing.is_empty() {
            return Err(ReceiveTokenError::InvalidInstruction.into());
        }
        Ok(parsed)
    }

    fn unpack_body<'a>(
        tag: u8,
        rest: &'a [u8],
        trailing: &mut &'a [u8],
    ) -> Result<Self, ProgramError> {
        Ok(match tag {
            0 => {
                let (&decimals, rest) = rest
                    .split_first()
                    .ok_or(ReceiveTokenError::InvalidInstruction)?;
                let (mint_authority, rest) = unpack_pubkey(rest)?;
                let (&fa_tag, rest) = rest
                    .split_first()
                    .ok_or(ReceiveTokenError::InvalidInstruction)?;
                let freeze_authority = match fa_tag {
                    0 => {
                        *trailing = rest;
                        None
                    }
                    1 => {
                        let (pk, next) = unpack_pubkey(rest)?;
                        *trailing = next;
                        Some(pk)
                    }
                    _ => return Err(ReceiveTokenError::InvalidInstruction.into()),
                };
                Self::InitializeMint2 {
                    decimals,
                    mint_authority,
                    freeze_authority,
                }
            }
            1 => {
                let (owner, next) = unpack_pubkey(rest)?;
                *trailing = next;
                Self::InitializeAccount3 { owner }
            }
            2 => {
                let (min_amount, rest) = unpack_u64(rest)?;
                let (&source_owner_mode, rest) = rest
                    .split_first()
                    .ok_or(ReceiveTokenError::InvalidInstruction)?;
                let (&recovery_authority_mode, rest) = rest
                    .split_first()
                    .ok_or(ReceiveTokenError::InvalidInstruction)?;
                let (recovery_authority, rest) = unpack_pubkey(rest)?;
                let (receipt_bond_lamports, rest) = unpack_u64(rest)?;
                let (receipt_ttl_slots, rest) = unpack_u64(rest)?;
                let (&allowlist_len, rest) = rest
                    .split_first()
                    .ok_or(ReceiveTokenError::InvalidInstruction)?;
                if allowlist_len as usize > ALLOWLIST_CAP {
                    return Err(ReceiveTokenError::AllowlistTooLarge.into());
                }
                let mut allowlist = Vec::with_capacity(allowlist_len as usize);
                let mut cursor = rest;
                for _ in 0..allowlist_len {
                    let (pk, next) = unpack_pubkey(cursor)?;
                    allowlist.push(pk);
                    cursor = next;
                }
                *trailing = cursor;
                Self::InitializeReceivePolicy {
                    min_amount,
                    source_owner_mode,
                    recovery_authority_mode,
                    recovery_authority,
                    receipt_bond_lamports,
                    receipt_ttl_slots,
                    allowlist,
                }
            }
            3 => {
                *trailing = rest;
                Self::EnsureGuard
            }
            4 => {
                let (amount, rest) = unpack_u64(rest)?;
                let (&decimals, rest) = rest
                    .split_first()
                    .ok_or(ReceiveTokenError::InvalidInstruction)?;
                if rest.len() < 32 {
                    return Err(ReceiveTokenError::InvalidInstruction.into());
                }
                let mut unique_nonce = [0u8; 32];
                unique_nonce.copy_from_slice(&rest[..32]);
                let (max_bond_lamports, rest) = unpack_u64(&rest[32..])?;
                let (max_ttl_slots, rest) = unpack_u64(rest)?;
                let (&max_recovery_mode, rest) = rest
                    .split_first()
                    .ok_or(ReceiveTokenError::InvalidInstruction)?;
                *trailing = rest;
                Self::TransferChecked {
                    amount,
                    decimals,
                    unique_nonce,
                    limits: HeldLimits {
                        max_bond_lamports,
                        max_ttl_slots,
                        max_recovery_mode,
                    },
                }
            }
            5 => {
                *trailing = rest;
                Self::ClaimReceipt
            }
            6 => {
                *trailing = rest;
                Self::CloseExpiredReceipt
            }
            7 => {
                let (amount, next) = unpack_u64(rest)?;
                *trailing = next;
                Self::MintTo { amount }
            }
            _ => return Err(ReceiveTokenError::InvalidInstruction.into()),
        })
    }

    pub fn pack(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        match self {
            Self::InitializeMint2 {
                decimals,
                mint_authority,
                freeze_authority,
            } => {
                buf.push(0);
                buf.push(*decimals);
                buf.extend_from_slice(mint_authority.as_ref());
                match freeze_authority {
                    None => buf.push(0),
                    Some(pk) => {
                        buf.push(1);
                        buf.extend_from_slice(pk.as_ref());
                    }
                }
            }
            Self::InitializeAccount3 { owner } => {
                buf.push(1);
                buf.extend_from_slice(owner.as_ref());
            }
            Self::InitializeReceivePolicy {
                min_amount,
                source_owner_mode,
                recovery_authority_mode,
                recovery_authority,
                receipt_bond_lamports,
                receipt_ttl_slots,
                allowlist,
            } => {
                buf.push(2);
                buf.extend_from_slice(&min_amount.to_le_bytes());
                buf.push(*source_owner_mode);
                buf.push(*recovery_authority_mode);
                buf.extend_from_slice(recovery_authority.as_ref());
                buf.extend_from_slice(&receipt_bond_lamports.to_le_bytes());
                buf.extend_from_slice(&receipt_ttl_slots.to_le_bytes());
                buf.push(allowlist.len() as u8);
                for pk in allowlist {
                    buf.extend_from_slice(pk.as_ref());
                }
            }
            Self::EnsureGuard => buf.push(3),
            Self::TransferChecked {
                amount,
                decimals,
                unique_nonce,
                limits,
            } => {
                buf.push(4);
                buf.extend_from_slice(&amount.to_le_bytes());
                buf.push(*decimals);
                buf.extend_from_slice(unique_nonce);
                buf.extend_from_slice(&limits.max_bond_lamports.to_le_bytes());
                buf.extend_from_slice(&limits.max_ttl_slots.to_le_bytes());
                buf.push(limits.max_recovery_mode);
            }
            Self::ClaimReceipt => buf.push(5),
            Self::CloseExpiredReceipt => buf.push(6),
            Self::MintTo { amount } => {
                buf.push(7);
                buf.extend_from_slice(&amount.to_le_bytes());
            }
        }
        buf
    }
}

/// Sender-declared ceilings on a held outcome.
///
/// Bounding cost alone is not enough: `max_recovery_mode` bounds *custody*. Under
/// `RecoveryAuthorityMode::Receiver` or `ThirdParty` the party that rejected the payment also
/// chooses who may claim it, so a sender that caps only the bond and the TTL has still handed the
/// destination discretion over the funds. The modes are ordered by how much the sender gives up:
/// `Originator`(0) keeps recovery with the sender, `Receiver`(1) and `ThirdParty`(2) do not.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeldLimits {
    pub max_bond_lamports: u64,
    pub max_ttl_slots: u64,
    pub max_recovery_mode: u8,
}

impl HeldLimits {
    /// Accept whatever the destination's policy says.
    pub fn unlimited() -> Self {
        Self {
            max_bond_lamports: u64::MAX,
            max_ttl_slots: u64::MAX,
            max_recovery_mode: RecoveryAuthorityMode::ThirdParty as u8,
        }
    }

    /// Refuse held delivery outright: a policy rejection becomes `failed`, not `held`.
    pub fn no_hold() -> Self {
        Self {
            max_bond_lamports: 0,
            max_ttl_slots: 0,
            max_recovery_mode: RecoveryAuthorityMode::Originator as u8,
        }
    }

    /// Accept a hold only if the sender itself remains the recovery authority.
    pub fn originator_recovery_only() -> Self {
        Self {
            max_recovery_mode: RecoveryAuthorityMode::Originator as u8,
            ..Self::unlimited()
        }
    }
}

fn unpack_pubkey(input: &[u8]) -> Result<(Pubkey, &[u8]), ProgramError> {
    if input.len() < 32 {
        return Err(ReceiveTokenError::InvalidInstruction.into());
    }
    let (key, rest) = input.split_at(32);
    let pk = Pubkey::new_from_array(key.try_into().unwrap());
    Ok((pk, rest))
}

fn unpack_u64(input: &[u8]) -> Result<(u64, &[u8]), ProgramError> {
    if input.len() < 8 {
        return Err(ReceiveTokenError::InvalidInstruction.into());
    }
    let (bytes, rest) = input.split_at(8);
    Ok((u64::from_le_bytes(bytes.try_into().unwrap()), rest))
}

// —— Instruction builders (client helpers) ——
//
// Positional arguments in account order, mirroring `spl_token::instruction::*` so the
// signatures read the way a Solana developer already expects and can be checked against the
// account lists in SPEC section 8. Unlike a constructor for persistent on-chain state, a
// mis-ordered call here does not survive: the program re-derives every PDA and re-checks every
// mint and owner, so a swap fails the transaction rather than writing a wrong record.

#[allow(clippy::too_many_arguments)]
pub fn initialize_mint2(
    program_id: &Pubkey,
    mint: &Pubkey,
    decimals: u8,
    mint_authority: &Pubkey,
    freeze_authority: Option<&Pubkey>,
) -> Instruction {
    let data = ReceiveTokenInstruction::InitializeMint2 {
        decimals,
        mint_authority: *mint_authority,
        freeze_authority: freeze_authority.copied(),
    }
    .pack();
    Instruction {
        program_id: *program_id,
        accounts: vec![AccountMeta::new(*mint, false)],
        data,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn initialize_account3(
    program_id: &Pubkey,
    account: &Pubkey,
    mint: &Pubkey,
    owner: &Pubkey,
) -> Instruction {
    let data = ReceiveTokenInstruction::InitializeAccount3 { owner: *owner }.pack();
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*account, false),
            AccountMeta::new_readonly(*mint, false),
        ],
        data,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn initialize_receive_policy(
    program_id: &Pubkey,
    token_account: &Pubkey,
    owner: &Pubkey,
    min_amount: u64,
    source_owner_mode: u8,
    recovery_authority_mode: u8,
    recovery_authority: Pubkey,
    receipt_bond_lamports: u64,
    receipt_ttl_slots: u64,
    allowlist: Vec<Pubkey>,
) -> Instruction {
    let data = ReceiveTokenInstruction::InitializeReceivePolicy {
        min_amount,
        source_owner_mode,
        recovery_authority_mode,
        recovery_authority,
        receipt_bond_lamports,
        receipt_ttl_slots,
        allowlist,
    }
    .pack();
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*token_account, false),
            AccountMeta::new_readonly(*owner, true),
        ],
        data,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn transfer_checked(
    program_id: &Pubkey,
    source: &Pubkey,
    mint: &Pubkey,
    destination: &Pubkey,
    authority: &Pubkey,
    amount: u64,
    decimals: u8,
    unique_nonce: [u8; 32],
    limits: HeldLimits,
    // When destination has ReceivePolicy, pass these; otherwise `None`.
    policy_accounts: Option<PolicyTransferAccounts>,
) -> Instruction {
    let data = ReceiveTokenInstruction::TransferChecked {
        amount,
        decimals,
        unique_nonce,
        limits,
    }
    .pack();
    let mut accounts = vec![
        AccountMeta::new(*source, false),
        AccountMeta::new_readonly(*mint, false),
        AccountMeta::new(*destination, false),
        AccountMeta::new_readonly(*authority, true),
    ];
    if let Some(p) = policy_accounts {
        accounts.push(AccountMeta::new(p.guard_token, false));
        accounts.push(AccountMeta::new(p.guard_state, false));
        accounts.push(AccountMeta::new(p.receipt, false));
        accounts.push(AccountMeta::new(p.bond_payer, true));
        accounts.push(AccountMeta::new_readonly(system_program::id(), false));
    }
    Instruction {
        program_id: *program_id,
        accounts,
        data,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PolicyTransferAccounts {
    pub guard_token: Pubkey,
    pub guard_state: Pubkey,
    pub receipt: Pubkey,
    pub bond_payer: Pubkey,
}

#[allow(clippy::too_many_arguments)]
pub fn claim_receipt(
    program_id: &Pubkey,
    receipt: &Pubkey,
    guard_token: &Pubkey,
    guard_state: &Pubkey,
    claim_destination: &Pubkey,
    mint: &Pubkey,
    claim_authority: &Pubkey,
    bond_dest: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*receipt, false),
            AccountMeta::new(*guard_token, false),
            AccountMeta::new(*guard_state, false),
            AccountMeta::new(*claim_destination, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new_readonly(*claim_authority, true),
            AccountMeta::new(*bond_dest, false),
        ],
        data: ReceiveTokenInstruction::ClaimReceipt.pack(),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn close_expired_receipt(
    program_id: &Pubkey,
    receipt: &Pubkey,
    guard_token: &Pubkey,
    guard_state: &Pubkey,
    source_owner_ata: &Pubkey,
    mint: &Pubkey,
    bond_dest: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*receipt, false),
            AccountMeta::new(*guard_token, false),
            AccountMeta::new(*guard_state, false),
            AccountMeta::new(*source_owner_ata, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new(*bond_dest, false),
        ],
        data: ReceiveTokenInstruction::CloseExpiredReceipt.pack(),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn mint_to(
    program_id: &Pubkey,
    mint: &Pubkey,
    account: &Pubkey,
    mint_authority: &Pubkey,
    amount: u64,
) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*mint, false),
            AccountMeta::new(*account, false),
            AccountMeta::new_readonly(*mint_authority, true),
        ],
        data: ReceiveTokenInstruction::MintTo { amount }.pack(),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn ensure_guard(
    program_id: &Pubkey,
    payer: &Pubkey,
    receiver: &Pubkey,
    mint: &Pubkey,
    guard_token: &Pubkey,
    guard_state: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: *program_id,
        accounts: vec![
            AccountMeta::new(*payer, true),
            AccountMeta::new_readonly(*receiver, false),
            AccountMeta::new_readonly(*mint, false),
            AccountMeta::new(*guard_token, false),
            AccountMeta::new(*guard_state, false),
            AccountMeta::new_readonly(system_program::id(), false),
        ],
        data: ReceiveTokenInstruction::EnsureGuard.pack(),
    }
}
