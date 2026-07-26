# Verification

How to reproduce host and on-VM evidence for `token-2022-receive`.

**Program ID:** `GyrTVV4hbcuzJuSz86FNq7K2UVAoSJQtcgHTVTz1hPPq`

## Suites

| Suite | Command | Role |
| --- | --- | --- |
| Smoke | `cargo test -p token-2022-receive --test smoke` | Policy/TLV/PDA unit checks |
| Guard custody | `cargo build-sbf` then `cargo test -p token-2022-receive --test guard_custody` | Held funds unspendable by the receiver; outcome return data |
| Policy bounds | `cargo build-sbf` then `cargo test -p token-2022-receive --test policy_bounds` | Write-once policy; mode validation; bond/TTL caps |
| Pre-funded PDAs | `cargo build-sbf` then `cargo test -p token-2022-receive --test prefunded_pdas` | Dust on a guard or receipt address cannot brick creation |
| Claim authority | `cargo build-sbf` then `cargo test -p token-2022-receive --test claim_authority` | All three recovery modes; unsigned authority; guard aliasing |
| Wire vectors | `cargo test -p token-2022-receive --test wire_vectors` | Byte vectors shared with the JS client |
| JS client | `cd clients/js && npm run typecheck && npm test` | Encoder vectors, pubkey length checks, account roles |
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

| Case | Outcome | Dest | Guard | CU (range) | Ceiling |
| --- | --- | --- | --- | --- | --- |
| BEFORE - no policy | credited | `1` | n/a | **2667** | 10_000 |
| AFTER - policy, dust | held | `0` | `1` | **12.1k - 18.1k** | 50_000 |
| AFTER - policy accepts `150` | credited | `150` | `0` | **7.1k - 13.1k** | 40_000 |
| AFTER - metas missing | failed `Custom(10)` | `0` | - | **2069** | 10_000 |
| Claim held dust | claim to dest | claim `1` | `0` | **7.7k - 15.2k** | 40_000 |
| Close expired | refund source ATA | - | `0` | **7.7k - 16.7k** | 40_000 |

Ranges are measured over repeated runs of the same binary, not across rebuilds. Every path that
derives a PDA varies by several thousand CU run to run because `find_program_address` searches
downward from bump 255 and the number of iterations depends on the keys involved, which the
fixtures generate randomly. The two paths that derive no PDA (`no-policy`, `missing-metas`) are
byte-stable, which is what identifies the variance as bump search rather than measurement noise.

Do not read a single sample as a regression or an improvement: only a shift in the whole range is
meaningful. `cu_ceilings` gates the ceilings, which hold across all observed runs.

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
| Pre-funded guard / receipt PDAs | `prefunded_pdas` (LiteSVM, real SBF) |
| Receiver / ThirdParty recovery modes, unsigned authority | `claim_authority` (LiteSVM, real SBF) |
| Guard aliased as its own payout | `claim_authority` |
| Zero-amount hold rejected | `guard_custody` |
| Non-canonical instruction encodings rejected | `smoke` |
| Client/program wire agreement (incl. variable-length allowlist) | `wire_vectors` + `clients/js/src/index.test.ts` |
| Foreign account extension rejected (SPEC section 9) | `smoke` |
| Error discriminants pinned to their numeric codes | `smoke` |
| Held requires an initialized guard_state; credited does not | `guard_custody` |
| Tokens donated straight into a guard are unattributed | `guard_custody` (pins a residual risk, not a defence) |

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
5. No CI workflow in-tree; both suites are run manually.
6. `GuardState` tracks an open-receipt count but not a held total, so
   `guard_token.amount >= sum(open receipts)` holds by construction rather than by assertion,
   and tokens sent directly into a guard are unattributed (see SPEC section 10 residual risks).
