//! Token mint / account base layouts (Token-2022–compatible first 82 / 165 bytes).
//! Packed manually (no Rust alignment padding) to match SPL Token sizes.

use crate::error::ReceiveTokenError;
use arrayref::{array_mut_ref, array_ref, array_refs, mut_array_refs};
use solana_program::{
    program_error::ProgramError,
    program_option::COption,
    program_pack::{IsInitialized, Pack, Sealed},
    pubkey::Pubkey,
};

pub const MINT_SIZE: usize = 82;
pub const ACCOUNT_SIZE: usize = 165;

#[repr(u8)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AccountState {
    #[default]
    Uninitialized = 0,
    Initialized = 1,
    Frozen = 2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Mint {
    pub mint_authority: COption<Pubkey>,
    pub supply: u64,
    pub decimals: u8,
    pub is_initialized: bool,
    pub freeze_authority: COption<Pubkey>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TokenAccount {
    pub mint: Pubkey,
    pub owner: Pubkey,
    pub amount: u64,
    pub delegate: COption<Pubkey>,
    pub state: AccountState,
    pub is_native: COption<u64>,
    pub delegated_amount: u64,
    pub close_authority: COption<Pubkey>,
}

impl TokenAccount {
    pub fn is_frozen(&self) -> bool {
        self.state == AccountState::Frozen
    }

    pub fn is_initialized(&self) -> bool {
        self.state == AccountState::Initialized || self.state == AccountState::Frozen
    }
}

impl Mint {
    pub fn is_initialized(&self) -> bool {
        self.is_initialized
    }
}

impl Sealed for Mint {}
impl Sealed for TokenAccount {}

impl IsInitialized for Mint {
    fn is_initialized(&self) -> bool {
        self.is_initialized
    }
}

impl IsInitialized for TokenAccount {
    fn is_initialized(&self) -> bool {
        TokenAccount::is_initialized(self)
    }
}

impl Pack for Mint {
    const LEN: usize = MINT_SIZE;

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let src = array_ref![src, 0, MINT_SIZE];
        let (mint_authority, supply, decimals, is_initialized, freeze_authority) =
            array_refs![src, 36, 8, 1, 1, 36];
        Ok(Mint {
            mint_authority: unpack_coption_key(mint_authority)?,
            supply: u64::from_le_bytes(*supply),
            decimals: decimals[0],
            is_initialized: match is_initialized {
                [0] => false,
                [1] => true,
                _ => return Err(ReceiveTokenError::InvalidAccountData.into()),
            },
            freeze_authority: unpack_coption_key(freeze_authority)?,
        })
    }

    fn pack_into_slice(&self, dst: &mut [u8]) {
        let dst = array_mut_ref![dst, 0, MINT_SIZE];
        let (mint_authority, supply, decimals, is_initialized, freeze_authority) =
            mut_array_refs![dst, 36, 8, 1, 1, 36];
        pack_coption_key(&self.mint_authority, mint_authority);
        *supply = self.supply.to_le_bytes();
        decimals[0] = self.decimals;
        is_initialized[0] = self.is_initialized as u8;
        pack_coption_key(&self.freeze_authority, freeze_authority);
    }
}

impl Pack for TokenAccount {
    const LEN: usize = ACCOUNT_SIZE;

    fn unpack_from_slice(src: &[u8]) -> Result<Self, ProgramError> {
        let src = array_ref![src, 0, ACCOUNT_SIZE];
        let (mint, owner, amount, delegate, state, is_native, delegated_amount, close_authority) =
            array_refs![src, 32, 32, 8, 36, 1, 12, 8, 36];
        Ok(TokenAccount {
            mint: Pubkey::new_from_array(*mint),
            owner: Pubkey::new_from_array(*owner),
            amount: u64::from_le_bytes(*amount),
            delegate: unpack_coption_key(delegate)?,
            state: match state {
                [0] => AccountState::Uninitialized,
                [1] => AccountState::Initialized,
                [2] => AccountState::Frozen,
                _ => return Err(ReceiveTokenError::InvalidAccountData.into()),
            },
            is_native: unpack_coption_u64(is_native)?,
            delegated_amount: u64::from_le_bytes(*delegated_amount),
            close_authority: unpack_coption_key(close_authority)?,
        })
    }

    fn pack_into_slice(&self, dst: &mut [u8]) {
        let dst = array_mut_ref![dst, 0, ACCOUNT_SIZE];
        let (mint, owner, amount, delegate, state, is_native, delegated_amount, close_authority) =
            mut_array_refs![dst, 32, 32, 8, 36, 1, 12, 8, 36];
        mint.copy_from_slice(self.mint.as_ref());
        owner.copy_from_slice(self.owner.as_ref());
        *amount = self.amount.to_le_bytes();
        pack_coption_key(&self.delegate, delegate);
        state[0] = self.state as u8;
        pack_coption_u64(&self.is_native, is_native);
        *delegated_amount = self.delegated_amount.to_le_bytes();
        pack_coption_key(&self.close_authority, close_authority);
    }
}

/// Decode a live mint, rejecting short buffers instead of aborting the program.
pub fn unpack_mint(data: &[u8]) -> Result<Mint, ProgramError> {
    if data.len() < MINT_SIZE {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    let mint = Mint::unpack_from_slice(&data[..MINT_SIZE])?;
    if !mint.is_initialized() {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    Ok(mint)
}

fn unpack_coption_key(src: &[u8; 36]) -> Result<COption<Pubkey>, ProgramError> {
    let (tag, body) = array_refs![src, 4, 32];
    match *tag {
        [0, 0, 0, 0] => Ok(COption::None),
        [1, 0, 0, 0] => Ok(COption::Some(Pubkey::new_from_array(*body))),
        _ => Err(ReceiveTokenError::InvalidAccountData.into()),
    }
}

fn pack_coption_key(src: &COption<Pubkey>, dst: &mut [u8; 36]) {
    let (tag, body) = mut_array_refs![dst, 4, 32];
    match src {
        COption::None => {
            *tag = [0; 4];
            *body = [0; 32];
        }
        COption::Some(key) => {
            *tag = [1, 0, 0, 0];
            body.copy_from_slice(key.as_ref());
        }
    }
}

fn unpack_coption_u64(src: &[u8; 12]) -> Result<COption<u64>, ProgramError> {
    let (tag, body) = array_refs![src, 4, 8];
    match *tag {
        [0, 0, 0, 0] => Ok(COption::None),
        [1, 0, 0, 0] => Ok(COption::Some(u64::from_le_bytes(*body))),
        _ => Err(ReceiveTokenError::InvalidAccountData.into()),
    }
}

fn pack_coption_u64(src: &COption<u64>, dst: &mut [u8; 12]) {
    let (tag, body) = mut_array_refs![dst, 4, 8];
    match src {
        COption::None => {
            *tag = [0; 4];
            *body = [0; 8];
        }
        COption::Some(val) => {
            *tag = [1, 0, 0, 0];
            *body = val.to_le_bytes();
        }
    }
}
