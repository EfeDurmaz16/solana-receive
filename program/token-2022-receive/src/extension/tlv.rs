//! Minimal TLV walker for post-base Token-2022-style account extensions.

use crate::constants::{ACCOUNT_TYPE_ACCOUNT, EXTENSION_TYPE_RECEIVE_POLICY};
use crate::error::ReceiveTokenError;
use crate::extension::receive_policy::ReceivePolicy;
use crate::state::{TokenAccount, ACCOUNT_SIZE};
use bytemuck::{bytes_of, bytes_of_mut, Zeroable};
use solana_program::{program_error::ProgramError, program_pack::Pack};

pub const TLV_TYPE_SIZE: usize = 2;
pub const TLV_LENGTH_SIZE: usize = 2;
pub const TLV_HEADER_SIZE: usize = TLV_TYPE_SIZE + TLV_LENGTH_SIZE;

pub fn find_extension_offset(data: &[u8], extension_type: u16) -> Result<usize, ProgramError> {
    if data.len() < ACCOUNT_SIZE + 1 {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    if data[ACCOUNT_SIZE] != ACCOUNT_TYPE_ACCOUNT {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    let mut cursor = ACCOUNT_SIZE + 1;
    while cursor + TLV_HEADER_SIZE <= data.len() {
        let typ = u16::from_le_bytes(data[cursor..cursor + 2].try_into().unwrap());
        let len = u16::from_le_bytes(data[cursor + 2..cursor + 4].try_into().unwrap()) as usize;
        let value_start = cursor + TLV_HEADER_SIZE;
        let value_end = value_start
            .checked_add(len)
            .ok_or(ReceiveTokenError::Overflow)?;
        if value_end > data.len() {
            return Err(ReceiveTokenError::InvalidAccountData.into());
        }
        if typ == extension_type {
            return Ok(value_start);
        }
        if typ == 0 {
            break;
        }
        cursor = value_end;
    }
    Err(ReceiveTokenError::PolicyNotEnabled.into())
}

/// Copy out policy (TLV value may be unaligned after the 165-byte base).
pub fn get_receive_policy(data: &[u8]) -> Result<ReceivePolicy, ProgramError> {
    let offset = find_extension_offset(data, EXTENSION_TYPE_RECEIVE_POLICY)?;
    let end = offset
        .checked_add(core::mem::size_of::<ReceivePolicy>())
        .ok_or(ReceiveTokenError::Overflow)?;
    if end > data.len() {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    let mut policy = ReceivePolicy::zeroed();
    bytes_of_mut(&mut policy).copy_from_slice(&data[offset..end]);
    Ok(policy)
}

pub fn has_receive_policy(data: &[u8]) -> bool {
    get_receive_policy(data).is_ok()
}

pub fn account_len_with_receive_policy() -> usize {
    ACCOUNT_SIZE + 1 + TLV_HEADER_SIZE + core::mem::size_of::<ReceivePolicy>()
}

pub fn write_receive_policy_tlv(
    data: &mut [u8],
    policy: &ReceivePolicy,
) -> Result<(), ProgramError> {
    let need = account_len_with_receive_policy();
    if data.len() < need {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    data[ACCOUNT_SIZE] = ACCOUNT_TYPE_ACCOUNT;
    let tlv_start = ACCOUNT_SIZE + 1;
    data[tlv_start..tlv_start + 2].copy_from_slice(&EXTENSION_TYPE_RECEIVE_POLICY.to_le_bytes());
    let len = core::mem::size_of::<ReceivePolicy>() as u16;
    data[tlv_start + 2..tlv_start + 4].copy_from_slice(&len.to_le_bytes());
    let value_start = tlv_start + TLV_HEADER_SIZE;
    data[value_start..value_start + len as usize].copy_from_slice(bytes_of(policy));
    Ok(())
}

pub fn unpack_account(data: &[u8]) -> Result<TokenAccount, ProgramError> {
    TokenAccount::unpack_from_slice(data)
}

pub fn pack_account(account: &TokenAccount, data: &mut [u8]) -> Result<(), ProgramError> {
    if data.len() < ACCOUNT_SIZE {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    account.pack_into_slice(data);
    Ok(())
}
