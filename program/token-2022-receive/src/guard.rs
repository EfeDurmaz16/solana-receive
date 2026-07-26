//! Receiver-scoped guard shard, keyed `(receiver, mint)`.
//!
//! Token-program scoping comes from the PDA's program id, not from a seed: these addresses
//! only exist under this program.

use crate::constants::{GUARD_SEED, GUARD_STATE_SEED, MAX_OPEN_RECEIPTS};
use crate::error::ReceiveTokenError;
use bytemuck::{Pod, Zeroable};
use solana_program::{
    account_info::AccountInfo, program_error::ProgramError, program_option::COption, pubkey::Pubkey,
};

/// Companion state PDA for open-receipt accounting (not the token account itself).
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct GuardState {
    pub discriminator: u64,
    pub receiver: Pubkey,
    pub mint: Pubkey,
    pub guard_token_account: Pubkey,
    pub open_receipts: u8,
    pub _padding: [u8; 7],
}

pub const GUARD_STATE_DISCRIMINATOR: u64 = 0x5245_4356_4755_4152; // "RECVGUAR"
pub const GUARD_STATE_SIZE: usize = core::mem::size_of::<GuardState>();

pub fn guard_token_seeds<'a>(receiver: &'a Pubkey, mint: &'a Pubkey) -> [&'a [u8]; 3] {
    [GUARD_SEED, receiver.as_ref(), mint.as_ref()]
}

pub fn guard_state_seeds<'a>(receiver: &'a Pubkey, mint: &'a Pubkey) -> [&'a [u8]; 3] {
    [GUARD_STATE_SEED, receiver.as_ref(), mint.as_ref()]
}

pub fn derive_guard_token_address(
    receiver: &Pubkey,
    mint: &Pubkey,
    program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(&guard_token_seeds(receiver, mint), program_id)
}

pub fn derive_guard_state_address(
    receiver: &Pubkey,
    mint: &Pubkey,
    program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(&guard_state_seeds(receiver, mint), program_id)
}

pub fn assert_guard_token_pda(
    account: &AccountInfo,
    receiver: &Pubkey,
    mint: &Pubkey,
    program_id: &Pubkey,
) -> Result<u8, ProgramError> {
    let (expected, bump) = derive_guard_token_address(receiver, mint, program_id);
    if account.key != &expected {
        return Err(ReceiveTokenError::InvalidPda.into());
    }
    Ok(bump)
}

pub fn assert_guard_state_pda(
    account: &AccountInfo,
    receiver: &Pubkey,
    mint: &Pubkey,
    program_id: &Pubkey,
) -> Result<u8, ProgramError> {
    let (expected, bump) = derive_guard_state_address(receiver, mint, program_id);
    if account.key != &expected {
        return Err(ReceiveTokenError::InvalidPda.into());
    }
    Ok(bump)
}

/// Is this token account a guard vault?
///
/// Guard custody has exactly two debit paths, `ClaimReceipt` and `CloseExpiredReceipt`, and both
/// pay out only against a receipt. Tokens that reach a guard any other way have no receipt and
/// are therefore unrecoverable by anyone, so every credit path has to refuse a guard rather than
/// report success for a transfer that destroyed the funds.
///
/// `close_authority` is set to the shard's receiver at creation purely as a marker (this program
/// has no CloseAccount and nothing else ever sets the field), so the common case costs one
/// `COption` discriminant check and the derivation runs only for an account that has one set.
pub fn is_guard_token_account(
    account: &crate::state::TokenAccount,
    key: &Pubkey,
    program_id: &Pubkey,
) -> bool {
    let COption::Some(receiver) = account.close_authority else {
        return false;
    };
    derive_guard_token_address(&receiver, &account.mint, program_id).0 == *key
}

/// Load a guard_state whose address is already PDA-asserted, checking that its *contents*
/// bind to the expected shard. Address alone is not enough: an uninitialized or mismatched
/// guard_state would otherwise be silently incremented on the held path.
pub fn load_guard_state(
    guard_state: &AccountInfo,
    guard_token: &Pubkey,
    receiver: &Pubkey,
    mint: &Pubkey,
) -> Result<(), ProgramError> {
    let data = guard_state.try_borrow_data()?;
    if data.len() < GUARD_STATE_SIZE {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    let gs = bytemuck::from_bytes::<GuardState>(&data[..GUARD_STATE_SIZE]);
    if gs.discriminator != GUARD_STATE_DISCRIMINATOR
        || gs.receiver != *receiver
        || gs.mint != *mint
        || gs.guard_token_account != *guard_token
    {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    Ok(())
}

impl GuardState {
    pub fn new(receiver: Pubkey, mint: Pubkey, guard_token_account: Pubkey) -> Self {
        Self {
            discriminator: GUARD_STATE_DISCRIMINATOR,
            receiver,
            mint,
            guard_token_account,
            open_receipts: 0,
            _padding: [0; 7],
        }
    }

    pub fn try_increment_open(&mut self) -> Result<(), ProgramError> {
        if self.open_receipts >= MAX_OPEN_RECEIPTS {
            return Err(ReceiveTokenError::GuardAtCapacity.into());
        }
        self.open_receipts = self
            .open_receipts
            .checked_add(1)
            .ok_or(ReceiveTokenError::Overflow)?;
        Ok(())
    }

    pub fn try_decrement_open(&mut self) -> Result<(), ProgramError> {
        self.open_receipts = self
            .open_receipts
            .checked_sub(1)
            .ok_or(ReceiveTokenError::Overflow)?;
        Ok(())
    }
}
