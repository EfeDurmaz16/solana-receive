//! Prototype defaults (documented for sRFC / README).

/// Default receipt TTL ≈ 7 days at ~400 ms/slot → 1_512_000 slots.
pub const DEFAULT_RECEIPT_TTL_SLOTS: u64 = 1_512_000;

/// Upper bound on receiver-chosen TTL ≈ 30 days. The receiver picks the TTL but the sender
/// pays for it in locked funds, so an unbounded TTL would let a destination hold a rejected
/// transfer hostage indefinitely under `Receiver` / `ThirdParty` recovery.
pub const MAX_RECEIPT_TTL_SLOTS: u64 = 6_480_000;

/// Upper bound on receiver-chosen receipt bond (1 SOL). The bond is debited from the
/// bond_payer signer, not from the receiver, so an unbounded value is a griefing lever.
pub const MAX_RECEIPT_BOND_LAMPORTS: u64 = 1_000_000_000;

/// Fixed-cap in-account source-owner allowlist (v0).
pub const ALLOWLIST_CAP: usize = 8;

/// Account type discriminator after the 165-byte Token Account base (Token-2022 style).
pub const ACCOUNT_TYPE_ACCOUNT: u8 = 2;
pub const ACCOUNT_TYPE_MINT: u8 = 1;

/// Extension type id for ReceivePolicy (custom; not in canonical Token-2022).
/// Chosen in the high range to avoid colliding with upstream `ExtensionType` values.
pub const EXTENSION_TYPE_RECEIVE_POLICY: u16 = 10_000;

/// Seeds
pub const GUARD_SEED: &[u8] = b"guard";
pub const GUARD_STATE_SEED: &[u8] = b"guard-state";
pub const RECEIPT_SEED: &[u8] = b"receipt";
