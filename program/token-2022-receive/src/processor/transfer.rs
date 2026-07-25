//! TransferChecked with credited vs held outcomes.

use crate::error::ReceiveTokenError;
use crate::extension::receive_policy::PolicyOutcome;
use crate::extension::tlv::{get_receive_policy, has_receive_policy, pack_account, unpack_account};
use crate::guard::{assert_guard_state_pda, assert_guard_token_pda, GuardState, GUARD_STATE_SIZE};
use crate::processor::require_signer;
use crate::receipt::{assert_receipt_pda, Receipt, RECEIPT_SIZE};
use crate::state::{Mint, MINT_SIZE};
use bytemuck::{bytes_of, from_bytes_mut};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    program::invoke_signed,
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
        has_receive_policy(&dest_data)
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

    let (dest_owner, dest_mint, policy_accepts) = {
        let dest_data = destination_info.try_borrow_data()?;
        let dest = unpack_account(&dest_data)?;
        if dest.is_frozen() {
            return Err(ReceiveTokenError::AccountFrozen.into());
        }
        if dest.mint != source_mint {
            return Err(ReceiveTokenError::MintMismatch.into());
        }
        let accepts = if dest_has_policy {
            let policy = get_receive_policy(&dest_data)?;
            policy.accepts(amount, &source_owner)
        } else {
            true
        };
        (dest.owner, dest.mint, accepts)
    };

    if source_info.key == destination_info.key {
        return Ok(());
    }

    if !dest_has_policy {
        move_amount(source_info, destination_info, amount, authority_info)?;
        return Ok(());
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

    let outcome = if policy_accepts {
        PolicyOutcome::Credited
    } else {
        PolicyOutcome::Held
    };

    match outcome {
        PolicyOutcome::Credited => {
            move_amount(source_info, destination_info, amount, authority_info)?;
            Ok(())
        }
        PolicyOutcome::Held => {
            let (policy_ttl, policy_bond, recovery_mode, recovery_authority) = {
                let dest_data = destination_info.try_borrow_data()?;
                let policy = get_receive_policy(&dest_data)?;
                (
                    policy.receipt_ttl_slots,
                    policy.receipt_bond_lamports,
                    policy.recovery_mode(),
                    policy.recovery_authority,
                )
            };

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
                .checked_add(policy_ttl)
                .ok_or(ReceiveTokenError::Overflow)?;

            let rent = Rent::get()?;
            let receipt_rent = rent.minimum_balance(RECEIPT_SIZE);
            let bond = policy_bond.max(receipt_rent);

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
                let receipt = Receipt::new(
                    amount,
                    dest_mint,
                    *source_info.key,
                    source_owner,
                    *destination_info.key,
                    dest_owner,
                    recovery_mode,
                    recovery_authority,
                    created_slot,
                    expires_slot,
                    bond,
                    *bond_payer.key,
                    unique_nonce,
                );
                rdata[..RECEIPT_SIZE].copy_from_slice(bytes_of(&receipt));
            }

            move_amount(source_info, guard_token, amount, authority_info)?;
            Ok(())
        }
    }
}

fn move_amount(
    source_info: &AccountInfo,
    dest_info: &AccountInfo,
    amount: u64,
    authority_info: &AccountInfo,
) -> ProgramResult {
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
