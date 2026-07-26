# Wire surface (v0, frozen for client codegen)

Normative behaviour lives in [SPEC.md](./SPEC.md). This page pins the **byte and account** contract that Codama / Kit clients must match. Authoritative executable pins: `program/token-2022-receive/tests/wire_vectors.rs`, `tests/smoke.rs` (error discriminants).

## Program

| | |
| --- | --- |
| Program ID | `GyrTVV4hbcuzJuSz86FNq7K2UVAoSJQtcgHTVTz1hPPq` |
| Instruction tag | single `u8` (Token-shaped), little-endian multi-byte fields |
| Origin | custom reference; **not** TokenzQd wire-compatible |

## Instruction tags

| Tag | Name | Data after tag |
| --- | --- | --- |
| 0 | `InitializeMint2` | `decimals:u8`, `mint_authority:Pubkey`, `freeze: Option<Pubkey>` (`0` / `1`+pk) |
| 1 | `InitializeAccount3` | `owner:Pubkey` |
| 2 | `InitializeReceivePolicy` | `min_amount:u64`, `source_owner_mode:u8`, `recovery_authority_mode:u8`, `recovery_authority:Pubkey`, `receipt_bond_lamports:u64`, `receipt_ttl_slots:u64`, `allowlist_len:u8`, `allowlist:Pubkey×N` (`N ≤ 8`) |
| 3 | `EnsureGuard` | (empty) |
| 4 | `TransferChecked` | **59 bytes total with tag**: `amount:u64`, `decimals:u8`, `unique_nonce:[u8;32]`, `max_bond_lamports:u64`, `max_ttl_slots:u64`, `max_recovery_mode:u8` |
| 5 | `ClaimReceipt` | (empty) |
| 6 | `CloseExpiredReceipt` | (empty) |
| 7 | `MintTo` | `amount:u64` |

Trailing bytes after a complete body are rejected (`InvalidInstruction`).

## Account metas

### `TransferChecked`

- **No policy / self-transfer:** 4 accounts — `source(w)`, `mint`, `destination(w)`, `authority(signer)`.
- **Policy destination (non-self):** 9 accounts — above + `guard_token(w)`, `guard_state(w)`, `receipt(w)`, `bond_payer(signer,w)`, `system_program`.
- Self-transfer is always the 4-account form even if the destination carries a policy (SPEC §8).
- The five policy accounts are **positional and all-or-nothing**. The generated builder marks them
  optional individually, so passing a subset shifts the rest into the wrong slots (a lone `receipt`
  lands in the `guard_token` position). Pass all five or none. `transferCheckedAccounts` in the
  client takes them as a single object for this reason.

### Other

| Ix | Accounts |
| --- | --- |
| `InitializeReceivePolicy` | `token_account(w)`, `owner(signer)` |
| `EnsureGuard` | `payer(signer,w)`, `receiver`, `mint`, `guard_token(w)`, `guard_state(w)`, `system_program` |
| `ClaimReceipt` | 7 — `receipt(w)`, `guard_token(w)`, `guard_state(w)`, `claim_destination(w)`, `mint`, `claim_authority(signer)`, `bond_dest(w)` |
| `CloseExpiredReceipt` | 6 — `receipt(w)`, `guard_token(w)`, `guard_state(w)`, `source_owner_token(w)`, `mint`, `bond_dest(w)` |

## PDAs

| Account | Seeds |
| --- | --- |
| Guard token | `b"guard" ‖ receiver ‖ mint` |
| Guard state | `b"guard-state" ‖ receiver ‖ mint` |
| Receipt | `b"receipt" ‖ receiver ‖ mint ‖ source_owner ‖ unique_nonce` |

## Outcomes

| Path | Tx | Return data | Log |
| --- | --- | --- | --- |
| credited | `Ok` | `[0]` | `Outcome: credited` |
| held | `Ok` | `[1]` | `Outcome: held receipt:` + receipt pubkey |
| failed | `Err` | (none) | ordinary program error |

Return data is last-instruction scoped. Multi-ix consumers should also index the held log / balances.

## Protocol caps (re-checked on held path)

| Constant | Value |
| --- | --- |
| `MAX_RECEIPT_BOND_LAMPORTS` | `1_000_000_000` |
| `MAX_RECEIPT_TTL_SLOTS` | `6_480_000` |
| `ALLOWLIST_CAP` | `8` |
| Effective bond | `max(policy.receipt_bond_lamports, rent_exempt(RECEIPT_SIZE))` |

## Errors

`ProgramError::Custom(n)` discriminants are pinned in `tests/smoke.rs` through `UnsupportedStateVersion = 32`. Gaps at 0, 5, 11, 12 and 16 are retired; do not reuse. (11 was `GuardAtCapacity`, dropped with the per-shard receipt cap.)
