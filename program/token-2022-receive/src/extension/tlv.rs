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

/// Locate a TLV entry, returning `(value_offset, declared_len)`.
///
/// `PolicyNotEnabled` means the account simply carries no such extension (a plain 165-byte
/// token account, or a TLV region that terminates without a match). Any other error means the
/// data is **malformed** and must not be treated as "no policy" — see [`has_receive_policy`].
pub fn find_extension_offset(
    data: &[u8],
    extension_type: u16,
) -> Result<(usize, usize), ProgramError> {
    if data.len() < ACCOUNT_SIZE + 1 || data[ACCOUNT_SIZE] != ACCOUNT_TYPE_ACCOUNT {
        return Err(ReceiveTokenError::PolicyNotEnabled.into());
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
            return Ok((value_start, len));
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
    let (offset, len) = find_extension_offset(data, EXTENSION_TYPE_RECEIVE_POLICY)?;
    // Honour the declared length: a shorter entry must not let the reader run into whatever
    // bytes follow it in the account.
    if len != core::mem::size_of::<ReceivePolicy>() {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    let end = offset.checked_add(len).ok_or(ReceiveTokenError::Overflow)?;
    if end > data.len() {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    let mut policy = ReceivePolicy::zeroed();
    bytes_of_mut(&mut policy).copy_from_slice(&data[offset..end]);
    Ok(policy)
}

/// `Ok(false)` only when the account genuinely carries no ReceivePolicy.
///
/// A malformed extension is an error, never a `false`: swallowing it would route the transfer
/// down the no-policy path and credit the destination, silently bypassing the very policy the
/// receiver attached. Presence is answered from the TLV header alone — no value copy.
pub fn has_receive_policy(data: &[u8]) -> Result<bool, ProgramError> {
    match find_extension_offset(data, EXTENSION_TYPE_RECEIVE_POLICY) {
        Ok((_, len)) if len == core::mem::size_of::<ReceivePolicy>() => Ok(true),
        Ok(_) => Err(ReceiveTokenError::InvalidAccountData.into()),
        Err(e) if e == ReceiveTokenError::PolicyNotEnabled.into() => Ok(false),
        Err(e) => Err(e),
    }
}

/// SPEC §9: ReceivePolicy does not coexist with other account extensions in v0.
///
/// Without this the claim was documentation only — the TLV walker happily skipped past any
/// other extension, so a Transfer Hook or Confidential Transfer account would have taken the
/// policy path with semantics this version never defined.
pub fn assert_no_other_extensions(data: &[u8]) -> Result<(), ProgramError> {
    if data.len() < ACCOUNT_SIZE + 1 || data[ACCOUNT_SIZE] != ACCOUNT_TYPE_ACCOUNT {
        return Ok(());
    }
    let mut cursor = ACCOUNT_SIZE + 1;
    while cursor + TLV_HEADER_SIZE <= data.len() {
        let typ = u16::from_le_bytes(data[cursor..cursor + 2].try_into().unwrap());
        if typ == 0 {
            return Ok(());
        }
        let len = u16::from_le_bytes(data[cursor + 2..cursor + 4].try_into().unwrap()) as usize;
        if typ != EXTENSION_TYPE_RECEIVE_POLICY {
            return Err(ReceiveTokenError::UnsupportedExtension.into());
        }
        cursor = (cursor + TLV_HEADER_SIZE)
            .checked_add(len)
            .ok_or(ReceiveTokenError::Overflow)?;
    }
    Ok(())
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
