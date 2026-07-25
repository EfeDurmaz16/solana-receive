//! Destination-account ReceivePolicy extension (v0).

use crate::constants::{ALLOWLIST_CAP, DEFAULT_RECEIPT_TTL_SLOTS};
use bytemuck::{Pod, Zeroable};
use solana_program::pubkey::Pubkey;

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

impl ReceivePolicy {
    pub fn source_owner_mode(&self) -> SourceOwnerMode {
        match self.source_owner_mode {
            1 => SourceOwnerMode::Allowlist,
            _ => SourceOwnerMode::AllowAll,
        }
    }

    pub fn recovery_mode(&self) -> RecoveryAuthorityMode {
        match self.recovery_authority_mode {
            1 => RecoveryAuthorityMode::Receiver,
            2 => RecoveryAuthorityMode::ThirdParty,
            _ => RecoveryAuthorityMode::Originator,
        }
    }

    pub fn allowlist_slice(&self) -> &[Pubkey] {
        let n = (self.allowlist_len as usize).min(ALLOWLIST_CAP);
        &self.allowlist[..n]
    }

    /// Pure policy evaluation: returns true if transfer should be **credited**.
    pub fn accepts(&self, amount: u64, source_owner: &Pubkey) -> bool {
        if amount < self.min_amount {
            return false;
        }
        match self.source_owner_mode() {
            SourceOwnerMode::AllowAll => true,
            SourceOwnerMode::Allowlist => self.allowlist_slice().iter().any(|k| k == source_owner),
        }
    }
}

/// Transfer outcome after policy evaluation (policy path only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyOutcome {
    Credited,
    Held,
}
