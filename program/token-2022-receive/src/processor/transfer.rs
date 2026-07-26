//! TransferChecked with credited vs held outcomes.

use crate::error::ReceiveTokenError;
use crate::extension::tlv::{
    assert_no_other_extensions, get_receive_policy, has_receive_policy, pack_account,
    unpack_account,
};
use crate::guard::{
    assert_guard_backed, assert_guard_state_pda, assert_guard_token_pda, is_guard_token_account,
    load_guard_state, GuardState, GUARD_STATE_SIZE,
};
use crate::instruction::HeldLimits;
use crate::processor::{create_pda_account, require_signer};
use crate::receipt::{
    assert_receipt_pda, Receipt, ReceiptStatus, RECEIPT_DISCRIMINATOR, RECEIPT_SIZE,
};
use crate::state::unpack_mint;
use bytemuck::{bytes_of, from_bytes_mut};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    clock::Clock,
    entrypoint::ProgramResult,
    msg,
    program::invoke,
    program::set_return_data,
    program_error::ProgramError,
    program_option::COption,
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
    limits: HeldLimits,
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
        let mint = unpack_mint(&mint_data)?;
        if mint.decimals != decimals {
            return Err(ReceiveTokenError::MintDecimalsMismatch.into());
        }
    }

    let (source_owner, source_mint, via_delegate) = {
        let source_data = source_info.try_borrow_data()?;
        let source = unpack_account(&source_data)?;
        if is_guard_token_account(&source, source_info.key, program_id) {
            return Err(ReceiveTokenError::GuardNotTransferable.into());
        }
        if source.is_frozen() {
            return Err(ReceiveTokenError::AccountFrozen.into());
        }
        if source.amount < amount {
            return Err(ReceiveTokenError::InsufficientFunds.into());
        }
        // Dispatch on the delegate arm first, matching SPL. The previous ordering preferred
        // the owner arm, then move_amount still decremented delegated_amount whenever the
        // authority happened to be the recorded delegate, underflowing an otherwise valid
        // owner-authorized transfer when an owner had delegated to itself.
        //
        // Approve / Revoke are not implemented in v0, so `delegate` is always None today and
        // this branch is unreachable. It is kept correct so the semantics are pinned for
        // whoever adds them.
        let via_delegate = match &source.delegate {
            COption::Some(delegate) if *delegate == *authority_info.key => {
                if source.delegated_amount < amount {
                    return Err(ReceiveTokenError::InsufficientFunds.into());
                }
                true
            }
            _ => {
                if source.owner != *authority_info.key {
                    return Err(ReceiveTokenError::OwnerMismatch.into());
                }
                false
            }
        };
        (source.owner, source.mint, via_delegate)
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
        // Both paths, not just the policy one: a guard never carries a policy, so the check
        // below in the policy branch could never see it as a destination. Tokens credited to a
        // guard have no receipt and no way out, and the instruction would otherwise report
        // `credited` for a transfer that destroyed them.
        if is_guard_token_account(&dest, destination_info.key, program_id) {
            return Err(ReceiveTokenError::GuardNotTransferable.into());
        }
        (dest.owner, dest.mint, policy)
    };

    if source_info.key == destination_info.key {
        // Nothing moves, but this is a success path and must report an outcome like the
        // others: integrators are told to read the byte rather than trust tx success.
        return credited();
    }

    if !dest_has_policy {
        move_amount(source_info, destination_info, amount, via_delegate)?;
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

    let policy = policy.ok_or(ReceiveTokenError::PolicyNotEnabled)?;

    if policy.accepts(amount, &source_owner)? {
        move_amount(source_info, destination_info, amount, via_delegate)?;
        credited()
    } else {
        // A zero-amount hold would burn one of MAX_OPEN_RECEIPTS slots while moving nothing,
        // letting someone with no tokens at all fill a victim's shard.
        if amount == 0 {
            return Err(ReceiveTokenError::InsufficientFunds.into());
        }

        // Validated here rather than before the accept/reject branch: only the held path
        // touches guard state, and requiring it on the credited path would make an otherwise
        // valid credit fail whenever EnsureGuard had not been run.
        load_guard_state(guard_state, guard_token.key, &dest_owner, &dest_mint)?;

        {
            let mut gs_data = guard_state.try_borrow_mut_data()?;
            if gs_data.len() < GUARD_STATE_SIZE {
                return Err(ReceiveTokenError::InvalidAccountData.into());
            }
            let gs = from_bytes_mut::<GuardState>(&mut gs_data[..GUARD_STATE_SIZE]);
            gs.record_hold(amount)?;
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

        // The sender's terms, checked before anything is debited. A destination can always
        // refuse a payment; it must not be able to set the price of being refused.
        if bond > limits.max_bond_lamports {
            return Err(ReceiveTokenError::BondAboveSenderLimit.into());
        }
        if policy.receipt_ttl_slots > limits.max_ttl_slots {
            return Err(ReceiveTokenError::TtlAboveSenderLimit.into());
        }
        if policy.recovery_authority_mode > limits.max_recovery_mode {
            return Err(ReceiveTokenError::RecoveryModeAboveSenderLimit.into());
        }

        let seeds: &[&[u8]] = &[
            crate::constants::RECEIPT_SEED,
            dest_owner.as_ref(),
            dest_mint.as_ref(),
            source_owner.as_ref(),
            unique_nonce.as_ref(),
            &[receipt_bump],
        ];
        create_pda_account(
            bond_payer,
            receipt_info,
            system_program,
            RECEIPT_SIZE,
            program_id,
            seeds,
        )?;
        // create_pda_account funds to rent exemption; top up to the policy bond if it is higher.
        if let Some(extra) = bond.checked_sub(receipt_info.lamports()) {
            if extra > 0 {
                invoke(
                    &system_instruction::transfer(bond_payer.key, receipt_info.key, extra),
                    &[
                        bond_payer.clone(),
                        receipt_info.clone(),
                        system_program.clone(),
                    ],
                )?;
            }
        }

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

        move_amount(source_info, guard_token, amount, via_delegate)?;

        // The deposit and the bookkeeping must agree before the instruction succeeds.
        {
            let gs_data = guard_state.try_borrow_data()?;
            let gs = bytemuck::from_bytes::<GuardState>(&gs_data[..GUARD_STATE_SIZE]);
            assert_guard_backed(guard_token, gs)?;
        }
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
    via_delegate: bool,
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
        if via_delegate {
            source.delegated_amount = source
                .delegated_amount
                .checked_sub(amount)
                .ok_or(ReceiveTokenError::Overflow)?;
            if source.delegated_amount == 0 {
                source.delegate = COption::None;
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
