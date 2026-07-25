//! Prototype defaults (documented for sRFC / README).

/// Maximum open held receipts per `(receiver, mint)` guard shard.
pub const MAX_OPEN_RECEIPTS: u8 = 64;

/// Default receipt TTL ≈ 7 days at ~400 ms/slot → 1_512_000 slots.
pub const DEFAULT_RECEIPT_TTL_SLOTS: u64 = 1_512_000;

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
