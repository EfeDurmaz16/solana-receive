//! Destination-account ReceivePolicy extension (v0).

use crate::constants::{ALLOWLIST_CAP, DEFAULT_RECEIPT_TTL_SLOTS};
use crate::error::ReceiveTokenError;
use bytemuck::{Pod, Zeroable};
use solana_program::{program_error::ProgramError, pubkey::Pubkey};

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceOwnerMode {
    AllowAll = 0,
    Allowlist = 1,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryAuthorityMode {
    /// Claim signer = receipt.source_owner
    Originator = 0,
    /// Claim signer = destination token-account owner (receiver)
    Receiver = 1,
    /// Claim signer = explicit pubkey
    ThirdParty = 2,
}

/// Fixed-size in-account policy (v0).
///
/// Size is constant so TLV length is known at init time.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct ReceivePolicy {
    pub min_amount: u64,
    /// `SourceOwnerMode` as u8
    pub source_owner_mode: u8,
    /// `RecoveryAuthorityMode` as u8
    pub recovery_authority_mode: u8,
    pub _padding: [u8; 6],
    pub recovery_authority: Pubkey,
    pub receipt_bond_lamports: u64,
    pub receipt_ttl_slots: u64,
    /// Number of populated allowlist entries (≤ ALLOWLIST_CAP)
    pub allowlist_len: u8,
    pub _padding2: [u8; 7],
    pub allowlist: [Pubkey; ALLOWLIST_CAP],
}

impl Default for ReceivePolicy {
    fn default() -> Self {
        Self {
            min_amount: 0,
            source_owner_mode: SourceOwnerMode::AllowAll as u8,
            recovery_authority_mode: RecoveryAuthorityMode::Originator as u8,
            _padding: [0; 6],
            recovery_authority: Pubkey::default(),
            receipt_bond_lamports: 0,
            receipt_ttl_slots: DEFAULT_RECEIPT_TTL_SLOTS,
            allowlist_len: 0,
            _padding2: [0; 7],
            allowlist: [Pubkey::default(); ALLOWLIST_CAP],
        }
    }
}

impl SourceOwnerMode {
    /// Fail closed: an unrecognised byte is a corrupt policy, not "allow everything".
    pub fn try_from_byte(b: u8) -> Result<Self, ProgramError> {
        match b {
            0 => Ok(Self::AllowAll),
            1 => Ok(Self::Allowlist),
            _ => Err(ReceiveTokenError::InvalidPolicyMode.into()),
        }
    }
}

impl RecoveryAuthorityMode {
    /// Fail closed: an unrecognised byte must not silently become `Originator`.
    pub fn try_from_byte(b: u8) -> Result<Self, ProgramError> {
        match b {
            0 => Ok(Self::Originator),
            1 => Ok(Self::Receiver),
            2 => Ok(Self::ThirdParty),
            _ => Err(ReceiveTokenError::InvalidPolicyMode.into()),
        }
    }
}

impl ReceivePolicy {
    pub fn source_owner_mode(&self) -> Result<SourceOwnerMode, ProgramError> {
        SourceOwnerMode::try_from_byte(self.source_owner_mode)
    }

    pub fn recovery_mode(&self) -> Result<RecoveryAuthorityMode, ProgramError> {
        RecoveryAuthorityMode::try_from_byte(self.recovery_authority_mode)
    }

    pub fn allowlist_slice(&self) -> &[Pubkey] {
        let n = (self.allowlist_len as usize).min(ALLOWLIST_CAP);
        &self.allowlist[..n]
    }

    /// Pure policy evaluation: `Ok(true)` if the transfer should be **credited**.
    pub fn accepts(&self, amount: u64, source_owner: &Pubkey) -> Result<bool, ProgramError> {
        if amount < self.min_amount {
            return Ok(false);
        }
        Ok(match self.source_owner_mode()? {
            SourceOwnerMode::AllowAll => true,
            SourceOwnerMode::Allowlist => self.allowlist_slice().iter().any(|k| k == source_owner),
        })
    }
}
