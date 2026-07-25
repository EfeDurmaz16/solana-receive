use num_enum::{IntoPrimitive, TryFromPrimitive};
use solana_program::program_error::ProgramError;
use thiserror::Error;

#[derive(Clone, Debug, Eq, Error, PartialEq, IntoPrimitive, TryFromPrimitive)]
#[repr(u32)]
pub enum ReceiveTokenError {
    #[error("Lamport balance below rent-exempt threshold")]
    NotRentExempt = 0,
    #[error("Insufficient funds")]
    InsufficientFunds,
    #[error("Mint mismatch")]
    MintMismatch,
    #[error("Account frozen")]
    AccountFrozen,
    #[error("Owner does not match")]
    OwnerMismatch,
    #[error("Fixed supply")]
    FixedSupply,
    #[error("Already in use")]
    AlreadyInUse,
    #[error("Invalid instruction data")]
    InvalidInstruction,
    #[error("Invalid account data")]
    InvalidAccountData,
    #[error("Decimals mismatch")]
    MintDecimalsMismatch,
    #[error("Missing required receive-policy accounts")]
    MissingPolicyAccounts,
    #[error("Receive policy rejected and guard shard is at capacity")]
    GuardAtCapacity,
    #[error("Transfer Hook coexistence forbidden in v0")]
    TransferHookForbidden,
    #[error("Receive policy not enabled on destination")]
    PolicyNotEnabled,
    #[error("Receipt not found or invalid")]
    InvalidReceipt,
    #[error("Receipt not expired")]
    ReceiptNotExpired,
    #[error("Receipt already closed")]
    ReceiptClosed,
    #[error("Unauthorized recovery / claim")]
    UnauthorizedClaim,
    #[error("Allowlist exceeds fixed cap")]
    AllowlistTooLarge,
    #[error("Overflow")]
    Overflow,
    #[error("Invalid PDA")]
    InvalidPda,
    #[error("Bond destination must be the recorded bond payer")]
    InvalidBondDestination,
    #[error("Unsupported extension combination")]
    UnsupportedExtension,
    #[error("Guard custody is not transferable outside claim / expiry")]
    GuardNotTransferable,
    #[error("Source and destination must differ")]
    SelfTransferForbidden,
}

impl From<ReceiveTokenError> for ProgramError {
    fn from(e: ReceiveTokenError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
