mod claim;
mod initialize;
mod transfer;

use crate::error::ReceiveTokenError;
use crate::instruction::ReceiveTokenInstruction;
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, msg, pubkey::Pubkey};

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
        } => {
            msg!("Instruction: TransferChecked");
            transfer::process_transfer_checked(program_id, accounts, amount, decimals, unique_nonce)
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
            initialize::process_mint_to(accounts, amount)
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
