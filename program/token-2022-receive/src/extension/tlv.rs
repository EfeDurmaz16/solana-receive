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

/// Walk the TLV region once, feeding each `(type, declared_len, value_offset)` to `visit`.
///
/// One walker, not two. Two independent parsers of the same bytes can disagree on a malformed
/// entry - one erroring where the other returns Ok - and which one a caller happens to reach
/// then decides whether a bad policy is rejected or silently treated as absent.
///
/// `visit` returning `Some(v)` stops the walk with `Ok(Some(v))`. Running out of entries is
/// `Ok(None)`; a declared length that overruns the account is an error.
fn walk_extensions<T>(
    data: &[u8],
    mut visit: impl FnMut(u16, usize, usize) -> Result<Option<T>, ProgramError>,
) -> Result<Option<T>, ProgramError> {
    if data.len() < ACCOUNT_SIZE + 1 || data[ACCOUNT_SIZE] != ACCOUNT_TYPE_ACCOUNT {
        return Ok(None);
    }
    let mut cursor = ACCOUNT_SIZE + 1;
    while cursor + TLV_HEADER_SIZE <= data.len() {
        let typ = u16::from_le_bytes(data[cursor..cursor + 2].try_into().unwrap());
        if typ == 0 {
            return Ok(None);
        }
        let len = u16::from_le_bytes(data[cursor + 2..cursor + 4].try_into().unwrap()) as usize;
        let value_start = cursor + TLV_HEADER_SIZE;
        let value_end = value_start
            .checked_add(len)
            .ok_or(ReceiveTokenError::Overflow)?;
        if value_end > data.len() {
            return Err(ReceiveTokenError::InvalidAccountData.into());
        }
        if let Some(found) = visit(typ, len, value_start)? {
            return Ok(Some(found));
        }
        cursor = value_end;
    }
    Ok(None)
}

/// Locate a TLV entry, returning `(value_offset, declared_len)`.
///
/// `PolicyNotEnabled` means the account simply carries no such extension (a plain 165-byte
/// token account, or a TLV region that terminates without a match). Any other error means the
/// data is **malformed** and must not be treated as "no policy" - see [`has_receive_policy`].
pub fn find_extension_offset(
    data: &[u8],
    extension_type: u16,
) -> Result<(usize, usize), ProgramError> {
    walk_extensions(data, |typ, len, offset| {
        Ok((typ == extension_type).then_some((offset, len)))
    })?
    .ok_or_else(|| ReceiveTokenError::PolicyNotEnabled.into())
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
/// receiver attached. Presence is answered from the TLV header alone - no value copy.
pub fn has_receive_policy(data: &[u8]) -> Result<bool, ProgramError> {
    match find_extension_offset(data, EXTENSION_TYPE_RECEIVE_POLICY) {
        Ok((_, len)) if len == core::mem::size_of::<ReceivePolicy>() => Ok(true),
        Ok(_) => Err(ReceiveTokenError::InvalidAccountData.into()),
        Err(e) if e == ReceiveTokenError::PolicyNotEnabled.into() => Ok(false),
        Err(e) => Err(e),
    }
}

/// SPEC section 9: ReceivePolicy does not coexist with other account extensions in v0.
///
/// Called on every policy-path destination AND on plain ones: an account carrying a foreign
/// extension but no ReceivePolicy would otherwise take the ordinary credit path, which is
/// exactly the case the claim needs to cover.
pub fn assert_no_other_extensions(data: &[u8]) -> Result<(), ProgramError> {
    walk_extensions(data, |typ, _, _| {
        if typ == EXTENSION_TYPE_RECEIVE_POLICY {
            Ok(None)
        } else {
            Err(ReceiveTokenError::UnsupportedExtension.into())
        }
    })
    .map(|_: Option<()>| ())
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

/// Decode a live token account.
///
/// `TokenAccount::unpack_from_slice` slice-indexes and would abort the program on a short
/// buffer, and an all-zero account parses as a valid `Uninitialized` one, which every value
/// path would then treat as spendable or creditable. Both are rejected here so callers get a
/// typed error rather than a panic or a phantom account.
pub fn unpack_account(data: &[u8]) -> Result<TokenAccount, ProgramError> {
    if data.len() < ACCOUNT_SIZE {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    let account = TokenAccount::unpack_from_slice(&data[..ACCOUNT_SIZE])?;
    if !account.is_initialized() {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    Ok(account)
}

pub fn pack_account(account: &TokenAccount, data: &mut [u8]) -> Result<(), ProgramError> {
    if data.len() < ACCOUNT_SIZE {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    account.pack_into_slice(data);
    Ok(())
}
