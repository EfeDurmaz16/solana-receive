# Verification

How to reproduce host and on-VM evidence for `token-2022-receive`.

**Program ID:** `GyrTVV4hbcuzJuSz86FNq7K2UVAoSJQtcgHTVTz1hPPq`

## Suites

| Suite | Command | Role |
| --- | --- | --- |
| Smoke | `cargo test -p token-2022-receive --test smoke` | Policy/TLV/PDA unit checks |
| Guard custody | `cargo build-sbf` then `cargo test -p token-2022-receive --test guard_custody` | Held funds unspendable by the receiver; outcome return data |
| Policy bounds | `cargo build-sbf` then `cargo test -p token-2022-receive --test policy_bounds` | Write-once policy; mode validation; bond/TTL caps |
| Host verify | `cargo test -p token-2022-receive --test verify_no_policy --test verify_policy_transfer --test verify_receipt_lifecycle` | Stateful AccountInfo + syscall stubs |
| Golden vectors | `cargo test -p token-2022-receive --test golden_vectors` | sRFC §9 IDs `V-NP`…`V-AU` + nonce contract |
| Upstream differential | `cargo test -p token-2022-receive --test upstream_differential` | Layout + no-policy overlap (not TokenzQd) |
| CU ceilings | `cargo build-sbf` then `cargo test -p token-2022-receive --test cu_ceilings -- --nocapture` | Regression alarms + tx footprint |
| LiteSVM | `cargo build-sbf` then `cargo test -p token-2022-receive --test litesvm_before_after --test litesvm_lifecycle -- --nocapture` | Real SBF + CU |
| Surfpool | `./scripts/surfpool-before-after.sh` | Optional RPC localnet (CLI may be absent) |

Full package:

```bash
export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
cargo test -p token-2022-receive
cargo build-sbf --manifest-path program/token-2022-receive/Cargo.toml
cargo test -p token-2022-receive --test litesvm_before_after --test litesvm_lifecycle -- --nocapture
```

Host stubs are not CU-faithful. Prefer LiteSVM for compute numbers.

## Before / after meaning

| Label | Setup | Dust below `min_amount` |
| --- | --- | --- |
| **BEFORE** | Destination **without** ReceivePolicy | Credits destination (ordinary Token semantics) |
| **AFTER** | Destination **with** ReceivePolicy | Succeeds as **held** (guard + receipt); destination unchanged |

Canonical `TokenzQd…` cannot host ReceivePolicy. “Before” is the no-policy path on this custom program ID.

## LiteSVM results (measured)

Same dust (`1`), same mint/source:

| Case | Outcome | Dest | Guard | CU | Ceiling |
| --- | --- | --- | --- | --- | --- |
| BEFORE - no policy | credited | `1` | n/a | **2253** | 10_000 |
| AFTER - policy, dust | held | `0` | `1` | **11210** | 50_000 |
| AFTER - policy accepts `150` | credited | `150` | `0` | **6637** | 40_000 |
| AFTER - metas missing | failed `Custom(10)` | `0` | - | **1810** | 10_000 |
| Claim held dust | claim → dest | claim `1` | `0` | **7225** | 40_000 |
| Close expired | refund source ATA | - | `0` | **8626** | 40_000 |

The held path costs less than the earlier `~14–21k` sample: the destination policy is decoded
once per transfer and presence is answered from the TLV header instead of copying the value.

CU drifts slightly across rebuilds; `cu_ceilings` enforces the ceilings above.  
Tests: `litesvm_before_after::*`, `litesvm_lifecycle::*`, `cu_ceilings::*`.

## Toolchain pin (reproducibility)

| Component | Version used when measuring |
| --- | --- |
| `solana-cli` / Agave | 4.1.1 |
| `solana-program` / `solana-sdk` | 2.2 |
| `litesvm` | 0.6.1 |
| `rustc` | 1.97.x |

**Mollusk:** not integrated. `mollusk-svm` 0.14 expects a newer Agave line than this 2.2 + LiteSVM 0.6.1 workspace; LiteSVM is the executable baseline.

## Contention

Distinct `(receiver, mint)` shards use distinct writable guard PDAs (no shared global guard). Same shard shares writable `guard_token` / `guard_state` and will serialize in a real scheduler. LiteSVM does not model multi-tx bank locks — runtime contention remains **unmeasured**; see `cu_ceilings::contention_account_lock_analysis_documented`.

## Host coverage

| Area | Status |
| --- | --- |
| No-policy credit / insufficient / decimals | Covered by host suites |
| Policy credited / held / missing metas | Covered by host suites |
| Claim + expiry close + pre-TTL reject | Covered by host suites |
| Claim wrong bond dest / wrong guard PDA | Covered by host suites |
| Instruction account counts (4 / 9 / 7 / 6) | Covered by host suites |
| Guard custody not spendable by receiver | `guard_custody` (LiteSVM, real SBF) |
| Policy write-once / mode validation / bond + TTL caps | `policy_bounds` (LiteSVM, real SBF) |
| Malformed / short TLV fails closed | `smoke` |

Error codes are explicit and stable (`ReceiveTokenError` discriminants); retired variants
leave documented gaps rather than renumbering live codes.

## Transaction footprint

| Instruction | Accounts | Data bytes |
| --- | --- | --- |
| `TransferChecked` (no policy) | 4 | 42 |
| `TransferChecked` (policy) | 9 | 42 |
| `ClaimReceipt` | 7 | 1 |
| `CloseExpiredReceipt` | 6 | 1 |

Single-ix footprints are under the 1232-byte tx limit. Guards are PDA-sharded by `(receiver, mint)`; receipts use `unique_nonce`.

## Surfpool (optional)

Install Surfpool from [releases](https://github.com/solana-foundation/surfpool/releases), then:

```bash
surfpool start --offline --skip-blockhash-check
# deploy target/deploy/token_2022_receive.so with a keypair matching declare_id!
```

Use when exercising RPC / Kit demos. For program semantics + CU, LiteSVM is enough.

## Gaps (not yet regression-gated)

1. Guard capacity / open-receipt ceiling under LiteSVM.
2. Scheduler-faithful multi-tx contention measurement (account-lock analysis only for now).
3. Surfpool + Kit client demo once CLI is installed.
4. Shard-fill griefing cost (filling `MAX_OPEN_RECEIPTS` denies further held delivery until
   receipts expire; see SPEC §10 residual risks).
5. JS client has no test suite and is not exercised in CI; there is no CI workflow in-tree.
