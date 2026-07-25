#![allow(dead_code)]
//! Shared host-test helpers (AccountInfo + syscall stubs).

use solana_program::{
    account_info::AccountInfo,
    clock::Clock,
    entrypoint::{ProgramResult, SUCCESS},
    instruction::Instruction,
    program_error::ProgramError,
    program_option::COption,
    program_pack::Pack,
    program_stubs::{set_syscall_stubs, SyscallStubs},
    pubkey::Pubkey,
    rent::Rent,
    system_instruction::SystemInstruction,
    system_program,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once, OnceLock};
use token_2022_receive::error::ReceiveTokenError;
use token_2022_receive::extension::receive_policy::ReceivePolicy;
use token_2022_receive::extension::tlv::{
    account_len_with_receive_policy, unpack_account, write_receive_policy_tlv,
};
use token_2022_receive::state::{AccountState, Mint, TokenAccount, ACCOUNT_SIZE, MINT_SIZE};

static SLOT: AtomicU64 = AtomicU64::new(1_000);
static STUB_LOCK: Mutex<()> = Mutex::new(());
static INSTALL: Once = Once::new();

struct HostStubs;

impl SyscallStubs for HostStubs {
    fn sol_get_clock_sysvar(&self, var_addr: *mut u8) -> u64 {
        let clock = Clock {
            slot: SLOT.load(Ordering::SeqCst),
            epoch_start_timestamp: 0,
            epoch: 0,
            leader_schedule_epoch: 0,
            unix_timestamp: 0,
        };
        unsafe {
            *(var_addr as *mut Clock) = clock;
        }
        SUCCESS
    }

    fn sol_get_rent_sysvar(&self, var_addr: *mut u8) -> u64 {
        unsafe {
            *(var_addr as *mut Rent) = Rent::default();
        }
        SUCCESS
    }

    fn sol_invoke_signed(
        &self,
        instruction: &Instruction,
        account_infos: &[AccountInfo],
        _signers_seeds: &[&[&[u8]]],
    ) -> ProgramResult {
        if instruction.program_id != system_program::id() {
            return Ok(());
        }
        let Ok(SystemInstruction::CreateAccount {
            lamports,
            space,
            owner,
        }) = bincode::deserialize(&instruction.data)
        else {
            return Ok(());
        };
        if instruction.accounts.len() < 2 {
            return Ok(());
        }
        let from_key = instruction.accounts[0].pubkey;
        let to_key = instruction.accounts[1].pubkey;
        let from = account_infos
            .iter()
            .find(|a| *a.key == from_key)
            .expect("create_account from");
        let to = account_infos
            .iter()
            .find(|a| *a.key == to_key)
            .expect("create_account to");

        let from_lamports = from.lamports();
        assert!(
            from_lamports >= lamports,
            "bond payer underfunded in harness"
        );
        **from.lamports.borrow_mut() = from_lamports - lamports;
        **to.lamports.borrow_mut() = to.lamports() + lamports;

        {
            let mut data = to.try_borrow_mut_data().unwrap();
            assert!(
                data.is_empty(),
                "create_account target must start empty in harness"
            );
            let ptr = data.as_mut_ptr();
            drop(data);
            unsafe {
                *to.data.borrow_mut() = std::slice::from_raw_parts_mut(ptr, space as usize);
            }
        }
        to.assign(&owner);
        Ok(())
    }
}

pub fn set_slot(slot: u64) {
    SLOT.store(slot, Ordering::SeqCst);
}

pub fn with_stubs<R>(f: impl FnOnce() -> R) -> R {
    INSTALL.call_once(|| {
        set_syscall_stubs(Box::new(HostStubs));
    });
    thread_local! {
        static DEPTH: Cell<u32> = const { Cell::new(0) };
    }
    DEPTH.with(|depth| {
        let d = depth.get();
        if d == 0 {
            let _guard = STUB_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            SLOT.store(1_000, Ordering::SeqCst);
            depth.set(1);
            let out = f();
            depth.set(0);
            out
        } else {
            depth.set(d + 1);
            let out = f();
            depth.set(d);
            out
        }
    })
}

pub fn program_id() -> Pubkey {
    token_2022_receive::id()
}

pub fn system_pid() -> &'static Pubkey {
    static ID: OnceLock<Pubkey> = OnceLock::new();
    ID.get_or_init(system_program::id)
}

pub fn err_custom(e: ReceiveTokenError) -> ProgramError {
    ProgramError::Custom(e as u32)
}

pub fn pack_mint(decimals: u8, authority: Pubkey) -> Vec<u8> {
    let mut data = vec![0u8; MINT_SIZE];
    Mint {
        mint_authority: COption::Some(authority),
        supply: 1_000_000,
        decimals,
        is_initialized: true,
        freeze_authority: COption::None,
    }
    .pack_into_slice(&mut data);
    data
}

pub fn pack_token(mint: Pubkey, owner: Pubkey, amount: u64) -> Vec<u8> {
    pack_token_with_state(mint, owner, amount, AccountState::Initialized)
}

pub fn pack_token_with_state(
    mint: Pubkey,
    owner: Pubkey,
    amount: u64,
    state: AccountState,
) -> Vec<u8> {
    let mut data = vec![0u8; ACCOUNT_SIZE];
    TokenAccount {
        mint,
        owner,
        amount,
        delegate: COption::None,
        state,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    }
    .pack_into_slice(&mut data);
    data
}

pub fn pack_policy_account(
    mint: Pubkey,
    owner: Pubkey,
    amount: u64,
    policy: &ReceivePolicy,
) -> Vec<u8> {
    let mut data = vec![0u8; account_len_with_receive_policy()];
    TokenAccount {
        mint,
        owner,
        amount,
        delegate: COption::None,
        state: AccountState::Initialized,
        is_native: COption::None,
        delegated_amount: 0,
        close_authority: COption::None,
    }
    .pack_into_slice(&mut data);
    write_receive_policy_tlv(&mut data, policy).unwrap();
    data
}

pub fn ai<'a>(
    key: &'a Pubkey,
    is_signer: bool,
    is_writable: bool,
    lamports: &'a mut u64,
    data: &'a mut [u8],
    owner: &'a Pubkey,
) -> AccountInfo<'a> {
    AccountInfo {
        key,
        is_signer,
        is_writable,
        lamports: Rc::new(RefCell::new(lamports)),
        data: Rc::new(RefCell::new(data)),
        owner,
        executable: false,
        rent_epoch: 0,
    }
}

pub fn empty_into(buf: &mut [u8]) -> &mut [u8] {
    let ptr = buf.as_mut_ptr();
    unsafe { std::slice::from_raw_parts_mut(ptr, 0) }
}

pub fn amount_of(data: &[u8]) -> u64 {
    unpack_account(data).unwrap().amount
}
