//! Claim and expiry paths for held receipts.

use crate::error::ReceiveTokenError;
use crate::extension::tlv::{pack_account, unpack_account};
use crate::guard::{
    assert_guard_state_pda, assert_guard_token_pda, load_guard_state, GuardState, GUARD_STATE_SIZE,
};
use crate::processor::require_signer;
use crate::receipt::{Receipt, ReceiptStatus, RECEIPT_DISCRIMINATOR, RECEIPT_SIZE};
use bytemuck::{from_bytes, from_bytes_mut};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    program_error::ProgramError,
    pubkey::Pubkey,
    sysvar::Sysvar,
};

fn load_open_receipt(
    receipt_info: &AccountInfo,
    program_id: &Pubkey,
) -> Result<Receipt, ProgramError> {
    if receipt_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    let data = receipt_info.try_borrow_data()?;
    if data.len() < RECEIPT_SIZE {
        return Err(ReceiveTokenError::InvalidReceipt.into());
    }
    let receipt = *from_bytes::<Receipt>(&data[..RECEIPT_SIZE]);
    if receipt.discriminator != RECEIPT_DISCRIMINATOR || !receipt.is_open() {
        return Err(ReceiveTokenError::InvalidReceipt.into());
    }
    Ok(receipt)
}

fn validate_guard_accounts(
    program_id: &Pubkey,
    guard_token: &AccountInfo,
    guard_state: &AccountInfo,
    receiver_owner: &Pubkey,
    mint: &Pubkey,
) -> Result<(), ProgramError> {
    assert_guard_token_pda(guard_token, receiver_owner, mint, program_id)?;
    assert_guard_state_pda(guard_state, receiver_owner, mint, program_id)?;
    load_guard_state(guard_state, guard_token.key, receiver_owner, mint)
}

fn require_bond_dest(bond_dest: &AccountInfo, bond_payer: &Pubkey) -> Result<(), ProgramError> {
    if bond_dest.key != bond_payer {
        return Err(ReceiveTokenError::InvalidBondDestination.into());
    }
    Ok(())
}

fn close_receipt_and_refund_bond(
    receipt_info: &AccountInfo,
    bond_dest: &AccountInfo,
) -> Result<(), ProgramError> {
    {
        let mut data = receipt_info.try_borrow_mut_data()?;
        let receipt = from_bytes_mut::<Receipt>(&mut data[..RECEIPT_SIZE]);
        receipt.status = ReceiptStatus::Closed as u8;
    }
    let lamports = receipt_info.lamports();
    **receipt_info.lamports.borrow_mut() = 0;
    **bond_dest.lamports.borrow_mut() = bond_dest
        .lamports()
        .checked_add(lamports)
        .ok_or(ReceiveTokenError::Overflow)?;
    {
        let mut data = receipt_info.try_borrow_mut_data()?;
        data.fill(0);
    }
    Ok(())
}

pub fn process_claim_receipt(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let receipt_info = next_account_info(account_info_iter)?;
    let guard_token = next_account_info(account_info_iter)?;
    let guard_state = next_account_info(account_info_iter)?;
    let claim_destination = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let claim_authority = next_account_info(account_info_iter)?;
    let bond_dest = next_account_info(account_info_iter)?;

    require_signer(claim_authority)?;

    let receipt = load_open_receipt(receipt_info, program_id)?;
    if receipt.claim_authority() != *claim_authority.key {
        return Err(ReceiveTokenError::UnauthorizedClaim.into());
    }
    if receipt.mint != *mint_info.key {
        return Err(ReceiveTokenError::MintMismatch.into());
    }
    require_bond_dest(bond_dest, &receipt.bond_payer)?;
    validate_guard_accounts(
        program_id,
        guard_token,
        guard_state,
        &receipt.receiver_owner,
        &receipt.mint,
    )?;

    {
        let dest_data = claim_destination.try_borrow_data()?;
        let dest = unpack_account(&dest_data)?;
        if dest.mint != receipt.mint {
            return Err(ReceiveTokenError::MintMismatch.into());
        }
        if dest.is_frozen() {
            return Err(ReceiveTokenError::AccountFrozen.into());
        }
    }

    {
        let mut gdata = guard_token.try_borrow_mut_data()?;
        let mut guard = unpack_account(&gdata)?;
        if guard.mint != receipt.mint {
            return Err(ReceiveTokenError::MintMismatch.into());
        }
        if guard.amount < receipt.amount {
            return Err(ReceiveTokenError::InsufficientFunds.into());
        }
        guard.amount = guard
            .amount
            .checked_sub(receipt.amount)
            .ok_or(ReceiveTokenError::Overflow)?;
        pack_account(&guard, &mut gdata)?;
    }
    {
        let mut ddata = claim_destination.try_borrow_mut_data()?;
        let mut dest = unpack_account(&ddata)?;
        dest.amount = dest
            .amount
            .checked_add(receipt.amount)
            .ok_or(ReceiveTokenError::Overflow)?;
        pack_account(&dest, &mut ddata)?;
    }

    {
        let mut gs_data = guard_state.try_borrow_mut_data()?;
        let gs = from_bytes_mut::<GuardState>(&mut gs_data[..GUARD_STATE_SIZE]);
        gs.try_decrement_open()?;
    }

    close_receipt_and_refund_bond(receipt_info, bond_dest)
}

pub fn process_close_expired_receipt(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let receipt_info = next_account_info(account_info_iter)?;
    let guard_token = next_account_info(account_info_iter)?;
    let guard_state = next_account_info(account_info_iter)?;
    let source_owner_ata = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let bond_dest = next_account_info(account_info_iter)?;

    let receipt = load_open_receipt(receipt_info, program_id)?;
    let clock = Clock::get()?;
    if clock.slot < receipt.expires_slot {
        return Err(ReceiveTokenError::ReceiptNotExpired.into());
    }
    if receipt.mint != *mint_info.key {
        return Err(ReceiveTokenError::MintMismatch.into());
    }
    require_bond_dest(bond_dest, &receipt.bond_payer)?;
    validate_guard_accounts(
        program_id,
        guard_token,
        guard_state,
        &receipt.receiver_owner,
        &receipt.mint,
    )?;

    {
        let ata_data = source_owner_ata.try_borrow_data()?;
        let ata = unpack_account(&ata_data)?;
        if ata.owner != receipt.source_owner {
            return Err(ReceiveTokenError::OwnerMismatch.into());
        }
        if ata.mint != receipt.mint {
            return Err(ReceiveTokenError::MintMismatch.into());
        }
        if ata.is_frozen() {
            return Err(ReceiveTokenError::AccountFrozen.into());
        }
    }

    {
        let mut gdata = guard_token.try_borrow_mut_data()?;
        let mut guard = unpack_account(&gdata)?;
        if guard.amount < receipt.amount {
            return Err(ReceiveTokenError::InsufficientFunds.into());
        }
        guard.amount = guard
            .amount
            .checked_sub(receipt.amount)
            .ok_or(ReceiveTokenError::Overflow)?;
        pack_account(&guard, &mut gdata)?;
    }
    {
        let mut adata = source_owner_ata.try_borrow_mut_data()?;
        let mut ata = unpack_account(&adata)?;
        ata.amount = ata
            .amount
            .checked_add(receipt.amount)
            .ok_or(ReceiveTokenError::Overflow)?;
        pack_account(&ata, &mut adata)?;
    }

    {
        let mut gs_data = guard_state.try_borrow_mut_data()?;
        let gs = from_bytes_mut::<GuardState>(&mut gs_data[..GUARD_STATE_SIZE]);
        gs.try_decrement_open()?;
    }

    close_receipt_and_refund_bond(receipt_info, bond_dest)
}
