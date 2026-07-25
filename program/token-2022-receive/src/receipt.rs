//! Held-delivery receipt accounts.

use crate::constants::RECEIPT_SEED;
use crate::error::ReceiveTokenError;
use crate::extension::RecoveryAuthorityMode;
use bytemuck::{Pod, Zeroable};
use solana_program::{account_info::AccountInfo, program_error::ProgramError, pubkey::Pubkey};

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptStatus {
    Open = 1,
    Closed = 2,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Receipt {
    pub discriminator: u64,
    pub amount: u64,
    pub mint: Pubkey,
    pub source_token_account: Pubkey,
    pub source_owner: Pubkey,
    pub destination_token_account: Pubkey,
    pub receiver_owner: Pubkey,
    pub recovery_authority_mode: u8,
    pub status: u8,
    pub _padding: [u8; 6],
    pub recovery_authority: Pubkey,
    pub created_slot: u64,
    pub expires_slot: u64,
    pub bond_lamports: u64,
    pub bond_payer: Pubkey,
    pub unique_nonce: [u8; 32],
}

pub const RECEIPT_DISCRIMINATOR: u64 = 0x5245_4356_5243_5054; // "RECVRCPT"
pub const RECEIPT_SIZE: usize = core::mem::size_of::<Receipt>();

pub fn receipt_seeds<'a>(
    receiver: &'a Pubkey,
    mint: &'a Pubkey,
    source_owner: &'a Pubkey,
    unique_nonce: &'a [u8; 32],
) -> [&'a [u8]; 5] {
    [
        RECEIPT_SEED,
        receiver.as_ref(),
        mint.as_ref(),
        source_owner.as_ref(),
        unique_nonce.as_ref(),
    ]
}

pub fn derive_receipt_address(
    receiver: &Pubkey,
    mint: &Pubkey,
    source_owner: &Pubkey,
    unique_nonce: &[u8; 32],
    program_id: &Pubkey,
) -> (Pubkey, u8) {
    Pubkey::find_program_address(
        &receipt_seeds(receiver, mint, source_owner, unique_nonce),
        program_id,
    )
}

pub fn assert_receipt_pda(
    account: &AccountInfo,
    receiver: &Pubkey,
    mint: &Pubkey,
    source_owner: &Pubkey,
    unique_nonce: &[u8; 32],
    program_id: &Pubkey,
) -> Result<u8, ProgramError> {
    let (expected, bump) =
        derive_receipt_address(receiver, mint, source_owner, unique_nonce, program_id);
    if account.key != &expected {
        return Err(ReceiveTokenError::InvalidPda.into());
    }
    Ok(bump)
}

impl Receipt {
    pub fn is_open(&self) -> bool {
        self.status == ReceiptStatus::Open as u8
    }

    pub fn claim_authority(&self) -> Pubkey {
        match self.recovery_authority_mode {
            x if x == RecoveryAuthorityMode::Receiver as u8 => self.receiver_owner,
            x if x == RecoveryAuthorityMode::ThirdParty as u8 => self.recovery_authority,
            _ => self.source_owner,
        }
    }
}
