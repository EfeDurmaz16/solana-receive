use crate::constants::{
    ALLOWLIST_CAP, DEFAULT_RECEIPT_TTL_SLOTS, MAX_RECEIPT_BOND_LAMPORTS, MAX_RECEIPT_TTL_SLOTS,
};
use crate::error::ReceiveTokenError;
use crate::extension::receive_policy::{ReceivePolicy, RecoveryAuthorityMode, SourceOwnerMode};
use crate::extension::tlv::{
    account_len_with_receive_policy, has_receive_policy, pack_account, unpack_account,
    write_receive_policy_tlv,
};
use crate::guard::{
    assert_guard_state_pda, assert_guard_token_pda, derive_guard_state_address,
    derive_guard_token_address, is_guard_token_account, GuardState, GUARD_STATE_DISCRIMINATOR,
    GUARD_STATE_SIZE,
};
use crate::processor::{create_pda_account, require_signer};
use crate::state::{unpack_mint, AccountState, Mint, TokenAccount, ACCOUNT_SIZE, MINT_SIZE};
use bytemuck::from_bytes_mut;
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    program_error::ProgramError,
    program_option::COption,
    program_pack::Pack,
    pubkey::Pubkey,
};

/// Refuse to initialize over an account this program already uses for something else.
///
/// Receipts and guard state carry a leading discriminator, so checking it stops either from being
/// reinterpreted as a mint or a token account and overwritten. Mints and token accounts have no
/// tag of their own; they are kept disjoint by size instead (see `process_initialize_mint2`).
fn reject_typed_account(data: &[u8]) -> ProgramResult {
    if data.len() < 8 {
        return Ok(());
    }
    let tag = u64::from_le_bytes(data[..8].try_into().unwrap());
    if tag == crate::receipt::RECEIPT_DISCRIMINATOR || tag == GUARD_STATE_DISCRIMINATOR {
        return Err(ReceiveTokenError::AlreadyInUse.into());
    }
    Ok(())
}

pub fn process_initialize_mint2(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    decimals: u8,
    mint_authority: Pubkey,
    freeze_authority: Option<Pubkey>,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let mint_info = next_account_info(account_info_iter)?;
    if mint_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    let mut data = mint_info.try_borrow_mut_data()?;
    // Exactly MINT_SIZE, not "at least". Neither mints nor token accounts carry a type tag, so a
    // mint allocated with >= ACCOUNT_SIZE bytes parses as an uninitialized token account and
    // InitializeAccount3 would overwrite it, bricking every token account of that mint. Pinning
    // the length keeps the two types disjoint by size. v0 defines no mint extensions.
    if data.len() != MINT_SIZE {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    reject_typed_account(&data)?;
    let existing = Mint::unpack_from_slice(&data[..MINT_SIZE])?;
    if existing.is_initialized() {
        return Err(ReceiveTokenError::AlreadyInUse.into());
    }
    let mint = Mint {
        mint_authority: COption::Some(mint_authority),
        supply: 0,
        decimals,
        is_initialized: true,
        freeze_authority: match freeze_authority {
            Some(pk) => COption::Some(pk),
            None => COption::None,
        },
    };
    mint.pack_into_slice(&mut data[..MINT_SIZE]);
    Ok(())
}

pub fn process_initialize_account3(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    owner: Pubkey,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let account_info = next_account_info(account_info_iter)?;
    let mint_info = next_account_info(account_info_iter)?;

    if account_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    if mint_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }

    let mint_data = mint_info.try_borrow_data()?;
    if mint_data.len() < MINT_SIZE {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    let _ = unpack_mint(&mint_data)?;

    let mut data = account_info.try_borrow_mut_data()?;
    if data.len() < ACCOUNT_SIZE {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    reject_typed_account(&data)?;
    let existing = TokenAccount::unpack_from_slice(&data[..ACCOUNT_SIZE])?;
    if existing.is_initialized() {
        return Err(ReceiveTokenError::AlreadyInUse.into());
    }
    let account = TokenAccount {
        mint: *mint_info.key,
        owner,
        amount: 0,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    };
    pack_account(&account, &mut data)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn process_initialize_receive_policy(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    min_amount: u64,
    source_owner_mode: u8,
    recovery_authority_mode: u8,
    recovery_authority: Pubkey,
    receipt_bond_lamports: u64,
    receipt_ttl_slots: u64,
    allowlist: Vec<Pubkey>,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let token_account_info = next_account_info(account_info_iter)?;
    let owner_info = next_account_info(account_info_iter)?;
    require_signer(owner_info)?;

    if token_account_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }
    if allowlist.len() > ALLOWLIST_CAP {
        return Err(ReceiveTokenError::AllowlistTooLarge.into());
    }
    // Parse the mode bytes here so an out-of-range value can never reach storage and decode
    // fail-open to AllowAll / Originator on the transfer path.
    SourceOwnerMode::try_from_byte(source_owner_mode)?;
    RecoveryAuthorityMode::try_from_byte(recovery_authority_mode)?;
    if receipt_bond_lamports > MAX_RECEIPT_BOND_LAMPORTS {
        return Err(ReceiveTokenError::PolicyBondTooLarge.into());
    }

    let mut data = token_account_info.try_borrow_mut_data()?;
    let account = unpack_account(&data)?;
    if account.owner != *owner_info.key {
        return Err(ReceiveTokenError::OwnerMismatch.into());
    }
    if !account.is_initialized() {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    if data.len() < account_len_with_receive_policy() {
        return Err(ReceiveTokenError::InvalidAccountData.into());
    }
    // A policy is write-once in v0. Rewriting it in place would let a receiver change
    // min_amount, recovery authority, bond and TTL between a sender's quote and the sender's
    // transaction - turning an accepted payment into a held one the receiver can claim.
    if has_receive_policy(&data)? {
        return Err(ReceiveTokenError::AlreadyInUse.into());
    }

    let ttl = if receipt_ttl_slots == 0 {
        DEFAULT_RECEIPT_TTL_SLOTS
    } else {
        receipt_ttl_slots
    };
    if ttl > MAX_RECEIPT_TTL_SLOTS {
        return Err(ReceiveTokenError::PolicyTtlTooLarge.into());
    }

    let mut policy = ReceivePolicy {
        min_amount,
        source_owner_mode,
        recovery_authority_mode,
        _padding: [0; 6],
        recovery_authority,
        receipt_bond_lamports,
        receipt_ttl_slots: ttl,
        allowlist_len: allowlist.len() as u8,
        _padding2: [0; 7],
        allowlist: [Pubkey::default(); ALLOWLIST_CAP],
    };
    for (i, pk) in allowlist.iter().enumerate() {
        policy.allowlist[i] = *pk;
    }

    write_receive_policy_tlv(&mut data, &policy)?;
    Ok(())
}

pub fn process_ensure_guard(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let payer = next_account_info(account_info_iter)?;
    let receiver = next_account_info(account_info_iter)?;
    let mint = next_account_info(account_info_iter)?;
    let guard_token = next_account_info(account_info_iter)?;
    let guard_state = next_account_info(account_info_iter)?;
    let system_program = next_account_info(account_info_iter)?;

    require_signer(payer)?;
    if *system_program.key != solana_program::system_program::id() {
        return Err(ProgramError::IncorrectProgramId);
    }

    let (expected_guard, guard_bump) =
        derive_guard_token_address(receiver.key, mint.key, program_id);
    if guard_token.key != &expected_guard {
        return Err(ReceiveTokenError::InvalidPda.into());
    }
    let (expected_state, state_bump) =
        derive_guard_state_address(receiver.key, mint.key, program_id);
    if guard_state.key != &expected_state {
        return Err(ReceiveTokenError::InvalidPda.into());
    }

    if guard_state.data_is_empty() {
        let seeds: &[&[u8]] = &[
            crate::constants::GUARD_STATE_SEED,
            receiver.key.as_ref(),
            mint.key.as_ref(),
            &[state_bump],
        ];
        create_pda_account(
            payer,
            guard_state,
            system_program,
            GUARD_STATE_SIZE,
            program_id,
            seeds,
        )?;
        let mut state_data = guard_state.try_borrow_mut_data()?;
        let state = from_bytes_mut::<GuardState>(&mut state_data[..GUARD_STATE_SIZE]);
        *state = GuardState::new(*receiver.key, *mint.key, *guard_token.key);
    }

    if guard_token.data_is_empty() {
        let seeds: &[&[u8]] = &[
            crate::constants::GUARD_SEED,
            receiver.key.as_ref(),
            mint.key.as_ref(),
            &[guard_bump],
        ];
        create_pda_account(
            payer,
            guard_token,
            system_program,
            ACCOUNT_SIZE,
            program_id,
            seeds,
        )?;
        // Held custody must NOT be spendable by the receiver - the receiver is exactly the
        // party the guard protects senders against. The token-level owner is the guard_state
        // PDA, which no external keypair can sign for, so the only debit paths are
        // ClaimReceipt / CloseExpiredReceipt.
        let account = TokenAccount {
            mint: *mint.key,
            owner: *guard_state.key,
            amount: 0,
            delegate: COption::None,
            state: AccountState::Initialized,
            is_native: COption::None,
            delegated_amount: 0,
            // Shard marker, not a close authority: this program has no CloseAccount, and
            // nothing else ever sets the field. Recording the receiver here lets any handler
            // recognise a guard in O(1) - see guard::is_guard_token_account.
            close_authority: COption::Some(*receiver.key),
        };
        let mut gdata = guard_token.try_borrow_mut_data()?;
        pack_account(&account, &mut gdata)?;
    } else {
        // An existing vault is repaired, not skipped. EnsureGuard used to only write these
        // fields when it created the account, so a vault created by an earlier build kept the
        // receiver as its token-level owner and carried no shard marker: still drainable
        // through the ordinary transfer path, and invisible to is_guard_token_account. Both
        // values are fixed by the PDA seeds, so this cannot be steered by a caller, and the
        // balance is left untouched.
        let mut gdata = guard_token.try_borrow_mut_data()?;
        let mut existing = unpack_account(&gdata)?;
        if existing.mint != *mint.key {
            return Err(ReceiveTokenError::MintMismatch.into());
        }
        let stale = existing.owner != *guard_state.key
            || existing.close_authority != COption::Some(*receiver.key)
            || existing.delegate != COption::None;
        if stale {
            existing.owner = *guard_state.key;
            existing.close_authority = COption::Some(*receiver.key);
            existing.delegate = COption::None;
            existing.delegated_amount = 0;
            pack_account(&existing, &mut gdata)?;
        }
    }

    let _ = assert_guard_token_pda(guard_token, receiver.key, mint.key, program_id)?;
    let _ = assert_guard_state_pda(guard_state, receiver.key, mint.key, program_id)?;
    Ok(())
}

pub fn process_mint_to(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    amount: u64,
) -> ProgramResult {
    let account_info_iter = &mut accounts.iter();
    let mint_info = next_account_info(account_info_iter)?;
    let account_info = next_account_info(account_info_iter)?;
    let authority_info = next_account_info(account_info_iter)?;
    require_signer(authority_info)?;

    if mint_info.owner != program_id || account_info.owner != program_id {
        return Err(ProgramError::IncorrectProgramId);
    }

    let mut mint_data = mint_info.try_borrow_mut_data()?;
    let mut mint = unpack_mint(&mint_data)?;
    match mint.mint_authority {
        COption::Some(auth) if auth == *authority_info.key => {}
        _ => return Err(ReceiveTokenError::OwnerMismatch.into()),
    }
    mint.supply = mint
        .supply
        .checked_add(amount)
        .ok_or(ReceiveTokenError::Overflow)?;
    mint.pack_into_slice(&mut mint_data[..MINT_SIZE]);

    let mut account_data = account_info.try_borrow_mut_data()?;
    let mut account = unpack_account(&account_data)?;
    if account.mint != *mint_info.key {
        return Err(ReceiveTokenError::MintMismatch.into());
    }
    if is_guard_token_account(&account, account_info.key, program_id) {
        return Err(ReceiveTokenError::GuardNotTransferable.into());
    }
    account.amount = account
        .amount
        .checked_add(amount)
        .ok_or(ReceiveTokenError::Overflow)?;
    pack_account(&account, &mut account_data)?;
    Ok(())
}
