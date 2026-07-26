mod claim;
mod initialize;
mod transfer;

use crate::error::ReceiveTokenError;
use crate::instruction::ReceiveTokenInstruction;
use solana_program::{
    account_info::AccountInfo,
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    pubkey::Pubkey,
    rent::Rent,
    system_instruction,
    sysvar::Sysvar,
};

pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    let instruction = ReceiveTokenInstruction::unpack(instruction_data)?;
    match instruction {
        ReceiveTokenInstruction::InitializeMint2 {
            decimals,
            mint_authority,
            freeze_authority,
        } => {
            msg!("Instruction: InitializeMint2");
            initialize::process_initialize_mint2(
                program_id,
                accounts,
                decimals,
                mint_authority,
                freeze_authority,
            )
        }
        ReceiveTokenInstruction::InitializeAccount3 { owner } => {
            msg!("Instruction: InitializeAccount3");
            initialize::process_initialize_account3(program_id, accounts, owner)
        }
        ReceiveTokenInstruction::InitializeReceivePolicy {
            min_amount,
            source_owner_mode,
            recovery_authority_mode,
            recovery_authority,
            receipt_bond_lamports,
            receipt_ttl_slots,
            allowlist,
        } => {
            msg!("Instruction: InitializeReceivePolicy");
            initialize::process_initialize_receive_policy(
                program_id,
                accounts,
                min_amount,
                source_owner_mode,
                recovery_authority_mode,
                recovery_authority,
                receipt_bond_lamports,
                receipt_ttl_slots,
                allowlist,
            )
        }
        ReceiveTokenInstruction::EnsureGuard => {
            msg!("Instruction: EnsureGuard");
            initialize::process_ensure_guard(program_id, accounts)
        }
        ReceiveTokenInstruction::TransferChecked {
            amount,
            decimals,
            unique_nonce,
            limits,
        } => {
            msg!("Instruction: TransferChecked");
            transfer::process_transfer_checked(
                program_id,
                accounts,
                amount,
                decimals,
                unique_nonce,
                limits,
            )
        }
        ReceiveTokenInstruction::ClaimReceipt => {
            msg!("Instruction: ClaimReceipt");
            claim::process_claim_receipt(program_id, accounts)
        }
        ReceiveTokenInstruction::CloseExpiredReceipt => {
            msg!("Instruction: CloseExpiredReceipt");
            claim::process_close_expired_receipt(program_id, accounts)
        }
        ReceiveTokenInstruction::MintTo { amount } => {
            msg!("Instruction: MintTo");
            initialize::process_mint_to(program_id, accounts, amount)
        }
    }
}

/// Shared signer check.
pub(crate) fn require_signer(info: &AccountInfo) -> ProgramResult {
    if !info.is_signer {
        return Err(ReceiveTokenError::OwnerMismatch.into());
    }
    Ok(())
}

/// Create a program-owned PDA that survives having been pre-funded.
///
/// `system_instruction::create_account` fails outright when the target already holds lamports.
/// Every PDA address here is derivable by anyone from public inputs, and anyone may credit
/// lamports to any address, so plain `create_account` lets one lamport of dust permanently
/// brick a guard shard (and with it all held delivery for that `(receiver, mint)` pair) or
/// block an individual receipt. Pre-funding leaves the account system-owned with no data —
/// only this program can sign for the PDA, so nobody else can assign or allocate it — which
/// is exactly the case `allocate` + `assign` handles.
pub(crate) fn create_pda_account<'a>(
    payer: &AccountInfo<'a>,
    target: &AccountInfo<'a>,
    system_program: &AccountInfo<'a>,
    space: usize,
    owner: &Pubkey,
    seeds: &[&[u8]],
) -> ProgramResult {
    let required = Rent::get()?.minimum_balance(space);
    let current = target.lamports();

    if current == 0 {
        return invoke_signed(
            &system_instruction::create_account(
                payer.key,
                target.key,
                required,
                space as u64,
                owner,
            ),
            &[payer.clone(), target.clone(), system_program.clone()],
            &[seeds],
        );
    }

    if let Some(shortfall) = required.checked_sub(current) {
        if shortfall > 0 {
            invoke(
                &system_instruction::transfer(payer.key, target.key, shortfall),
                &[payer.clone(), target.clone(), system_program.clone()],
            )?;
        }
    }
    invoke_signed(
        &system_instruction::allocate(target.key, space as u64),
        &[target.clone(), system_program.clone()],
        &[seeds],
    )?;
    invoke_signed(
        &system_instruction::assign(target.key, owner),
        &[target.clone(), system_program.clone()],
        &[seeds],
    )
}
