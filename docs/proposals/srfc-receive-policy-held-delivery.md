# sRFC Draft: Destination Token-Account Receive Policy with Held Delivery

| Field | Value |
| --- | --- |
| **Title** | Destination Token-Account Receive Policy with Non-Reverting Held Delivery |
| **Status** | Draft for discussion (pre-number assignment) |
| **Created** | 2026-07-26 |
| **Related** | [token-2022#124](https://github.com/solana-program/token-2022/issues/124) |
| **Normative** | [`../SPEC.md`](../SPEC.md) |
| **Evidence** | [`../VERIFICATION.md`](../VERIFICATION.md) |
| **Non-claims** | Not a SIMD. No legacy Tokenkeg USDC/USDT interception. Not ambient enforcement on unmodified Token-2022. |

## Abstract

Propose an opt-in **destination token-account extension** that evaluates receiver policy during transfer processing and yields `failed`, `credited`, or `held`.

On policy reject of an otherwise token-valid transfer, the instruction **does not revert**. Funds route to a **receiver-scoped guard** with a **receipt** for claim or expiry recovery.

This differs from mint Transfer Hooks and from [token-2022#124](https://github.com/solana-program/token-2022/issues/124) account-side hooks, which **revert** on disapproval.

## 1. Problem

Receivers lack a protocol-native combination of:

1. Receiver-controlled inbound policy on a destination token account,
2. Non-reverting rejection, and
3. Attributable recovery (claim / expiry).

Without (2)+(3), reject either reverts the whole transfer (breaking composable senders) or forces a cooperative app vault that ordinary ATA transfers never enter.

## 2. Why existing pieces are insufficient

| Mechanism | Gap |
| --- | --- |
| Mint Transfer Hook | Issuer-owned; typically reverts; hook accounts often read-only — cannot redirect into a guard alone |
| Memo / freeze / default account state | Partial; not held recovery |
| Permanent delegate | Issuer power, not receiver inbound filter |
| App `deposit()` vault | Cooperative only; ATA path bypasses it |

## 3. Prior art: #124 vs held delivery

[#124](https://github.com/solana-program/token-2022/issues/124) explores per-account hook programs that **approve or reject**. On disapproval the transfer **halts**. Maintainer notes on mint allow-flags and tx-size complexity still apply.

| Dimension | #124-style account hook | This proposal |
| --- | --- | --- |
| On disapproval | Transfer **reverts** | Transfer **succeeds**; funds **held** |
| Fund destination on reject | Unchanged (no credit) | Guard + receipt |
| Recovery UX | N/A (sender still holds funds) | Claim / expiry |
| Implementation locus | Hook CPI | Transfer processing routes funds |

#124 is valuable for account-side autonomy. It does **not** provide non-reverting held delivery.

## 4. Semantics

Normative detail: [`../SPEC.md`](../SPEC.md). Summary:

- Scope = destination token account for one mint.
- v0 rules = `min_amount` + source-**owner** membership (allowlist cap 8).
- Outcomes = `failed` \| `credited` \| `held`.
- Guard shard = `(receiver_owner, mint)` — no global guard.
- Receipts = bounded (64), bonded, TTL, full-claim only; expiry returns to source-owner same-mint account.
- Policy transfer = **9** accounts; missing metas → `failed`.
- Transfer Hook coexistence unsupported in v0 reference.

## 5. Account resolution

No-policy destinations keep today’s 4-account shape.  
Policy-enabled destinations require guard_token, guard_state, receipt, bond_payer, system_program (see SPEC §8). Clients must detect the extension and resolve metas; post-success decoding should distinguish credited vs held.

## 6. Security

Fail-closed metas; per-receiver shards; depositor-funded bond + capacity + TTL; membership = source owner; explicit non-claim of USDC/USDT interception. Full table in SPEC §10.

## 7. Compatibility

See SPEC §9. Confidential Transfer incompatible in v0. Legacy Tokenkeg out of scope.

## 8. Compute / size (measured on reference program)

Every path that derives a PDA varies by several thousand CU run to run, because `find_program_address` iterates from bump 255 and the count depends on the keys involved. Ranges below are measured over repeated runs; a single sample is not a performance guarantee, and only a shift in the whole range indicates a real change. See [`../VERIFICATION.md`](../VERIFICATION.md).

| Path | Accounts | CU (LiteSVM, range) | Ceiling |
| --- | --- | --- | --- |
| No-policy transfer | 4 | 2.7k | 10_000 |
| Policy credited | 9 | 7.1k - 13.1k | 40_000 |
| Policy held | 9 | 12.1k - 18.1k | 50_000 |
| Missing metas | 4 (incomplete) | 2.1k | 10_000 |
| Claim held dust | 7 | 7.7k - 15.2k | 40_000 |
| Close expired | 6 | 7.7k - 16.7k | 40_000 |

Serialized policy-transfer tx ≈ **540B** (&lt; 1232). Contention: distinct receivers do not share a writable guard; same `(receiver, mint)` serializes. Mollusk not integrated on this toolchain.

## 9. Reference vectors

| ID | Expected | Executable test |
| --- | --- | --- |
| `V-NP` | No extension → ordinary credit | `golden_vectors::v_np_no_policy_credits_destination` |
| `V-CR` | Policy accept → credited | `golden_vectors::v_cr_policy_accept_credits_destination` |
| `V-HD` | Policy reject → held | `golden_vectors::v_hd_policy_reject_holds_to_guard` |
| `V-FL` | Missing metas / insufficient / capacity → failed | `v_fl_*` |
| `V-CL` | Claim full / wrong authority | `v_cl_*` |
| `V-EX` | Expiry close / pre-TTL reject | `v_ex_*` |
| `V-AU` | Allowlist membership uses source owner | `v_au_allowlist_uses_source_owner` |

Also covered: `unique_nonce` distinct PDAs + collision → `AlreadyInUse`.  
Upstream overlap (layout + no-policy amounts, not TokenzQd execution): `upstream_differential`.

Run: `cargo test -p token-2022-receive --test golden_vectors --test upstream_differential`.

## 10. Proposal path

1. This sRFC + SPEC + reference program (custom program ID).  
2. [Maintainer discussion](./maintainer-discussion.md) (#124 revert vs held).  
3. [SIMD outline](./simd-conditional-outline.md) **only after** canonical/runtime confirmation.

## 11. Canonical open questions

See [`decision-request.md`](./decision-request.md): mint allow-flag; account-resolution shape; greenfield reference vs upstream fork; whether a SIMD is required. Reference defaults are locked in SPEC §4.
