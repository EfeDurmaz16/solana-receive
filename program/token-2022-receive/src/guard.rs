//! Receiver-scoped guard shard, keyed `(receiver, mint)`.
//!
//! Token-program scoping comes from the PDA's program id, not from a seed: these addresses
//! only exist under this program.

use crate::constants::{GUARD_SEED, GUARD_STATE_SEED};
use crate::error::ReceiveTokenError;
use bytemuck::{Pod, Zeroable};
use solana_program::{
    account_info::AccountInfo, program_error::ProgramError, program_option::COption, pubkey::Pubkey,
};

/// Companion state PDA for held-custody accounting (not the token account itself).
///
/// `held_amount` is the sum of the amounts of every open receipt in this shard. It exists so the
/// custody invariant `guard_token.amount >= held_amount` can be **asserted** after each mutation
/// rather than merely holding by construction: if any future change lets tokens leave the guard
/// without releasing a receipt, or lets a receipt be written without a matching deposit, the very
/// next settlement fails closed instead of paying someone else's balance out.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct GuardState {
    pub discriminator: u64,
    /// Layout version. Present so a future field can be added with a migration instead of
    /// silently reinterpreting live shards: without it, a size change is only detectable as a
    /// length error, and a same-size change is not detectable at all.
    pub version: u8,
    pub _padding: [u8; 7],
    pub receiver: Pubkey,
    pub mint: Pubkey,
    pub guard_token_account: Pubkey,
    /// Observability only: no instruction branches on it. Kept because an indexer or an operator
    /// needs to know a shard has outstanding obligations without scanning for receipt PDAs.
    pub open_receipts: u64,
    pub held_amount: u64,
}

pub const GUARD_STATE_DISCRIMINATOR: u64 = 0x5245_4356_4755_4152; // "RECVGUAR"
pub const GUARD_STATE_VERSION: u8 = 1;
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

/// Assert the custody invariant: the vault covers every open receipt in its shard.
///
/// Checked after each mutation on both the deposit and the settlement paths, so a divergence
/// surfaces where it is introduced rather than as one sender being unable to claim later.
pub fn assert_guard_backed(
    guard_token: &AccountInfo,
    state: &GuardState,
) -> Result<(), ProgramError> {
    let guard = crate::extension::tlv::unpack_account(&guard_token.try_borrow_data()?)?;
    if guard.amount < state.held_amount {
        return Err(ReceiveTokenError::GuardUnderfunded.into());
    }
    Ok(())
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
    if gs.version != GUARD_STATE_VERSION {
        return Err(ReceiveTokenError::UnsupportedStateVersion.into());
    }
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
            version: GUARD_STATE_VERSION,
            _padding: [0; 7],
            receiver,
            mint,
            guard_token_account,
            open_receipts: 0,
            held_amount: 0,
        }
    }

    /// Account for a new held receipt.
    ///
    /// There is deliberately no per-shard receipt cap. A cap here would be a shared,
    /// permissionless resource: anyone could fill it and deny every other sender held delivery
    /// until the receipts expired, at a cost of nothing but refundable bond. It protected no
    /// one in exchange, because the bond payer funds each receipt's rent (never the receiver)
    /// and no instruction ever enumerates receipts. Each receipt being self-funding is the
    /// actual defence against rent griefing.
    pub fn record_hold(&mut self, amount: u64) -> Result<(), ProgramError> {
        self.open_receipts = self
            .open_receipts
            .checked_add(1)
            .ok_or(ReceiveTokenError::Overflow)?;
        self.held_amount = self
            .held_amount
            .checked_add(amount)
            .ok_or(ReceiveTokenError::Overflow)?;
        Ok(())
    }

    /// Account for a receipt being claimed or expired.
    pub fn record_release(&mut self, amount: u64) -> Result<(), ProgramError> {
        self.open_receipts = self
            .open_receipts
            .checked_sub(1)
            .ok_or(ReceiveTokenError::Overflow)?;
        self.held_amount = self
            .held_amount
            .checked_sub(amount)
            .ok_or(ReceiveTokenError::Overflow)?;
        Ok(())
    }
}
