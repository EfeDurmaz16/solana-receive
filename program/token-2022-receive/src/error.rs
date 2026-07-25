use num_enum::{IntoPrimitive, TryFromPrimitive};
use solana_program::program_error::ProgramError;
use thiserror::Error;

/// Discriminants are explicit and stable: they surface to clients as
/// `ProgramError::Custom(n)` and are quoted in `docs/VERIFICATION.md`. Gaps at 0, 5, 12 and 16
/// are retired variants; reuse them only if the meaning matches.
#[derive(Clone, Debug, Eq, Error, PartialEq, IntoPrimitive, TryFromPrimitive)]
#[repr(u32)]
pub enum ReceiveTokenError {
    #[error("Insufficient funds")]
    InsufficientFunds = 1,
    #[error("Mint mismatch")]
    MintMismatch = 2,
    #[error("Account frozen")]
    AccountFrozen = 3,
    #[error("Owner does not match")]
    OwnerMismatch = 4,
    #[error("Already in use")]
    AlreadyInUse = 6,
    #[error("Invalid instruction data")]
    InvalidInstruction = 7,
    #[error("Invalid account data")]
    InvalidAccountData = 8,
    #[error("Decimals mismatch")]
    MintDecimalsMismatch = 9,
    #[error("Missing required receive-policy accounts")]
    MissingPolicyAccounts = 10,
    #[error("Receive policy rejected and guard shard is at capacity")]
    GuardAtCapacity = 11,
    #[error("Receive policy not enabled on destination")]
    PolicyNotEnabled = 13,
    #[error("Receipt not found or invalid")]
    InvalidReceipt = 14,
    #[error("Receipt not expired")]
    ReceiptNotExpired = 15,
    #[error("Unauthorized recovery / claim")]
    UnauthorizedClaim = 17,
    #[error("Allowlist exceeds fixed cap")]
    AllowlistTooLarge = 18,
    #[error("Overflow")]
    Overflow = 19,
    #[error("Invalid PDA")]
    InvalidPda = 20,
    #[error("Bond destination must be the recorded bond payer")]
    InvalidBondDestination = 21,
    #[error("Receive policy cannot coexist with other account extensions in v0")]
    UnsupportedExtension = 22,
    #[error("Guard custody is not transferable outside claim / expiry")]
    GuardNotTransferable = 23,
    #[error("Source and destination must differ")]
    SelfTransferForbidden = 24,
    #[error("Unrecognized receive-policy mode byte")]
    InvalidPolicyMode = 25,
    #[error("Receipt bond exceeds the protocol maximum")]
    PolicyBondTooLarge = 26,
    #[error("Receipt TTL exceeds the protocol maximum")]
    PolicyTtlTooLarge = 27,
}

impl From<ReceiveTokenError> for ProgramError {
    fn from(e: ReceiveTokenError) -> Self {
        ProgramError::Custom(e as u32)
    }
}
