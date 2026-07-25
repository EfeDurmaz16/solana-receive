//! Token-2022-shaped **reference program** with destination-account **ReceivePolicy** held delivery.
//!
//! Custom program ID (not canonical TokenzQd…). Extends transfer processing itself —
//! this is **not** a mint-side Transfer Hook. Instruction tags are reference-local
//! (not wire-compatible with upstream Token-2022 discriminators).
//!
//! Upstream layout/semantics inspiration: `solana-program/token-2022` @ `5f64085`
//! (see `.upstream-token-2022-sha.txt`). v0 deliberately omits confidential transfer,
//! transfer fees, and Transfer Hook coexistence.

#![allow(clippy::arithmetic_side_effects)]

pub mod constants;
pub mod error;
pub mod extension;
pub mod guard;
pub mod instruction;
pub mod processor;
pub mod receipt;
pub mod state;

use solana_program::declare_id;

declare_id!("GyrTVV4hbcuzJuSz86FNq7K2UVAoSJQtcgHTVTz1hPPq");

#[cfg(not(feature = "no-entrypoint"))]
use solana_program::entrypoint;

#[cfg(not(feature = "no-entrypoint"))]
entrypoint!(process_instruction);

pub use processor::process_instruction;
