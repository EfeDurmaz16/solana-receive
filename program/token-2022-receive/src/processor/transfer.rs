//! TransferChecked with credited vs held outcomes.

use crate::error::ReceiveTokenError;
use crate::extension::tlv::{
    assert_no_other_extensions, get_receive_policy, has_receive_policy, pack_account,
    unpack_account,
};
use crate::guard::{
    assert_guard_state_pda, assert_guard_token_pda, load_guard_state, GuardState, GUARD_STATE_SIZE,
};
use crate::processor::require_signer;
use crate::receipt::{
    assert_receipt_pda, Receipt, ReceiptStatus, RECEIPT_DISCRIMINATOR, RECEIPT_SIZE,
};
use crate::state::{Mint, MINT_SIZE};
use bytemuck::{bytes_of, from_bytes_mut};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    msg,
    program::invoke_signed,
    program::set_return_data,
    program_error::ProgramError,
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};

pub fn process_transfer_checked(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
    decimals: u8,
    unique_nonce: [u8; 32],
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let source_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;
    let destination_info = next_account_info(account_info_iter)?;
    let authority_info = next_account_info(account_info_iter)?;

    require_signer(authority_info)?;

    if source_info.owner != program_id || destination_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    if mint_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }

    let dest_has_policy = {
        let dest_data = destination_info.try_borrow_data()?;
        has_receive_policy(&dest_data)?
    };

    {
        let mint_data = mint_info.try_borrow_data()?;
        let mint = Mint::unpack_from_slice(&mint_data[..MINT_SIZE])?;
        if mint.decimals != decimals {
            return Err(ReceiveTokenError::MintDecimalsMismatch.into());
        }
    }

    let (source_owner, source_mint) = {
        let source_data = source_info.try_borrow_data()?;
        let source = unpack_account(&source_data)?;
        if source.is_frozen() {
            return Err(ReceiveTokenError::AccountFrozen.into());
        }
        if source.amount < amount {
            return Err(ReceiveTokenError::InsufficientFunds.into());
        }
        let authorized = match (&source.delegate, source.owner == *authority_info.key) {
            (_, true) => true,
            (COption::Some(delegate), false)
                if *delegate == *authority_info.key && source.delegated_amount >= amount =>
            {
                true
            }
            _ => false,
        };
        if !authorized {
            return Err(ReceiveTokenError::OwnerMismatch.into());
        }
        (source.owner, source.mint)
    };

    if source_mint != *mint_info.key {
        return Err(ReceiveTokenError::MintMismatch.into());
    }

    // Read the destination once: the policy decision and every field the held branch needs
    // come from the same borrow, so no second decode can disagree with the first.
    let (dest_owner, dest_mint, policy) = {
        let dest_data = destination_info.try_borrow_data()?;
        let dest = unpack_account(&dest_data)?;
        if dest.is_frozen() {
            return Err(ReceiveTokenError::AccountFrozen.into());
        }
        if dest.mint != source_mint {
            return Err(ReceiveTokenError::MintMismatch.into());
        }
        let policy = if dest_has_policy {
            assert_no_other_extensions(&dest_data)?;
            Some(get_receive_policy(&dest_data)?)
        } else {
            None
        };
        (dest.owner, dest.mint, policy)
    };

    if source_info.key == destination_info.key {
        return Ok(());
    }

    if !dest_has_policy {
        move_amount(source_info, destination_info, amount, authority_info)?;
        return credited();
    }

    let guard_token = next_account_info(account_info_iter)
        .map_err(|_| ReceiveTokenError::MissingPolicyAccounts)?;
    let guard_state = next_account_info(account_info_iter)
        .map_err(|_| ReceiveTokenError::MissingPolicyAccounts)?;
    let receipt_info = next_account_info(account_info_iter)
        .map_err(|_| ReceiveTokenError::MissingPolicyAccounts)?;
    let bond_payer = next_account_info(account_info_iter)
        .map_err(|_| ReceiveTokenError::MissingPolicyAccounts)?;
    let system_program = next_account_info(account_info_iter)
        .map_err(|_| ReceiveTokenError::MissingPolicyAccounts)?;

    require_signer(bond_payer)?;
    if *system_program.key != solana_program::system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    assert_guard_token_pda(guard_token, &dest_owner, &dest_mint, program_id)?;
    assert_guard_state_pda(guard_state, &dest_owner, &dest_mint, program_id)?;

    load_guard_state(guard_state, guard_token.key, &dest_owner, &dest_mint)?;

    // Defense in depth: the guard is never a transfer source. Unreachable while the guard's
    // token-level owner is a PDA, but pinned here so an owner-field regression cannot silently
    // re-open receipt minting against undeposited guard balance.
    if source_info.key == guard_token.key || destination_info.key == guard_token.key {
        return Err(ReceiveTokenError::GuardNotTransferable.into());
    }

    let policy = policy.ok_or(ReceiveTokenError::PolicyNotEnabled)?;

    if policy.accepts(amount, &source_owner)? {
        move_amount(source_info, destination_info, amount, authority_info)?;
        credited()
    } else {
        {
            let mut gs_data = guard_state.try_borrow_mut_data()?;
            if gs_data.len() < GUARD_STATE_SIZE {
                return Err(ReceiveTokenError::InvalidAccountData.into());
            }
            let gs = from_bytes_mut::<GuardState>(&mut gs_data[..GUARD_STATE_SIZE]);
            gs.try_increment_open()?;
        }

        let receipt_bump = assert_receipt_pda(
            receipt_info,
            &dest_owner,
            &dest_mint,
            &source_owner,
            &unique_nonce,
            program_id,
        )?;

        if !receipt_info.data_is_empty() {
            return Err(ReceiveTokenError::AlreadyInUse.into());
        }

        let clock = Clock::get()?;
        let created_slot = clock.slot;
        let expires_slot = created_slot
            .checked_add(policy.receipt_ttl_slots)
            .ok_or(ReceiveTokenError::Overflow)?;

        let rent = Rent::get()?;
        let receipt_rent = rent.minimum_balance(RECEIPT_SIZE);
        let bond = policy.receipt_bond_lamports.max(receipt_rent);

        let seeds: &[&[u8]] = &[
            crate::constants::RECEIPT_SEED,
            dest_owner.as_ref(),
            dest_mint.as_ref(),
            source_owner.as_ref(),
            unique_nonce.as_ref(),
            &[receipt_bump],
        ];
        invoke_signed(
            &system_instruction::create_account(
                bond_payer.key,
                receipt_info.key,
                bond,
                RECEIPT_SIZE as u64,
                program_id,
            ),
            &[
                bond_payer.clone(),
                receipt_info.clone(),
                system_program.clone(),
            ],
            &[seeds],
        )?;

        {
            let mut rdata = receipt_info.try_borrow_mut_data()?;
            // Named fields, not 13 positional arguments: two adjacent Pubkey pairs here
            // (source/receiver owner, source/destination account) would swap silently.
            let receipt = Receipt {
                discriminator: RECEIPT_DISCRIMINATOR,
                amount,
                mint: dest_mint,
                source_token_account: *source_info.key,
                source_owner,
                destination_token_account: *destination_info.key,
                receiver_owner: dest_owner,
                recovery_authority_mode: policy.recovery_mode()? as u8,
                status: ReceiptStatus::Open as u8,
                _padding: [0; 6],
                recovery_authority: policy.recovery_authority,
                created_slot,
                expires_slot,
                bond_lamports: bond,
                bond_payer: *bond_payer.key,
                unique_nonce,
            };
            rdata[..RECEIPT_SIZE].copy_from_slice(bytes_of(&receipt));
        }

        move_amount(source_info, guard_token, amount, authority_info)?;
        held()
    }
}

/// Transfer outcome, reported as instruction return data.
///
/// `held` succeeds, so a caller that only checks "did the transaction land" would read a
/// diverted payment as a delivered one. The outcome byte makes the distinction machine
/// readable without decoding the destination account.
#[repr(u8)]
pub enum TransferOutcome {
    Credited = 0,
    Held = 1,
}

fn credited() -> ProgramResult {
    msg!("Outcome: credited");
    set_return_data(&[TransferOutcome::Credited as u8]);
    Ok(())
}

fn held() -> ProgramResult {
    msg!("Outcome: held");
    set_return_data(&[TransferOutcome::Held as u8]);
    Ok(())
}

fn move_amount(
    source_info: &AccountInfo,
    dest_info: &AccountInfo,
    amount: u64,
    authority_info: &AccountInfo,
) -> ProgramResult {
    // Aliased source/destination would debit and credit the same buffer in two separate
    // borrows and cancel to a silent no-op, which callers read as "the tokens moved".
    if source_info.key == dest_info.key {
        return Err(ReceiveTokenError::SelfTransferForbidden.into());
    }
    {
        let mut source_data = source_info.try_borrow_mut_data()?;
        let mut source = unpack_account(&source_data)?;
        source.amount = source
            .amount
            .checked_sub(amount)
            .ok_or(ReceiveTokenError::Overflow)?;
        if let COption::Some(delegate) = source.delegate {
            if delegate == *authority_info.key {
                source.delegated_amount = source
                    .delegated_amount
                    .checked_sub(amount)
                    .ok_or(ReceiveTokenError::Overflow)?;
                if source.delegated_amount == 0 {
                    source.delegate = COption::None;
                }
            }
        }
        pack_account(&source, &mut source_data)?;
    }
    {
        let mut dest_data = dest_info.try_borrow_mut_data()?;
        let mut dest = unpack_account(&dest_data)?;
        dest.amount = dest
            .amount
            .checked_add(amount)
            .ok_or(ReceiveTokenError::Overflow)?;
        pack_account(&dest, &mut dest_data)?;
    }
    Ok(())
}
