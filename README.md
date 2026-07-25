# solana-receive

Receiver-side **held delivery** for Token-2022-shaped token accounts: an opt-in destination `ReceivePolicy` that can credit, fail, or route non-matching transfers into a receiver-scoped guard with a claimable receipt — without reverting the outer transfer for policy rejection alone.

Greenfield **custom program ID** reference. Not canonical Token-2022. **Not wire-compatible** with `TokenzQd…` instruction tags. Does not intercept legacy Tokenkeg USDC/USDT.

## Why

Ordinary SPL / Token-2022 transfers credit a destination token account (usually an ATA). Receivers lack a protocol-native way to:

1. Attach inbound acceptance rules to that account,
2. Keep the transfer successful when policy rejects, and
3. Recover rejected amounts via claim or expiry.

Mint Transfer Hooks are issuer-owned and typically revert. Account-side hooks discussed in [token-2022#124](https://github.com/solana-program/token-2022/issues/124) are approve/reject (revert). App `deposit()` vaults are cooperative rails — ordinary ATA transfers never enter them.

## Outcomes

```mermaid
flowchart TD
  A[Transfer to policy-enabled destination] --> B{Token-valid?}
  B -->|No| F[failed]
  B -->|Yes| C{Policy accepts?}
  C -->|Yes| D[credited → destination]
  C -->|No| H[held → guard + receipt; Ok]
```

| Outcome | Meaning |
| --- | --- |
| `failed` | Ordinary token / account-resolution failure (atomic; no receipt) |
| `credited` | Policy accepts → `source → destination` |
| `held` | Policy rejects → `source → guard` + receipt; instruction `Ok` |

## Status / non-claims

| | |
| --- | --- |
| In tree | Reference program, host + LiteSVM suites, golden vectors, draft sRFC |
| Measured | CU ceilings + footprints under LiteSVM (see [docs/VERIFICATION.md](docs/VERIFICATION.md)) |
| Not claimed | Canonical Token-2022, TokenzQd wire compatibility, legacy USDC interception, full upstream extension parity, mainnet product |

## Tests

```bash
export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
cargo test -p token-2022-receive
cargo build-sbf --manifest-path program/token-2022-receive/Cargo.toml
cargo test -p token-2022-receive --test litesvm_before_after --test litesvm_lifecycle --test cu_ceilings -- --nocapture
```

## Layout

```
program/token-2022-receive/   # Reference program (custom program ID)
clients/js/                   # Minimal instruction/PDA helpers
docs/SPEC.md                  # Normative v0 semantics
docs/VERIFICATION.md          # How to re-run evidence + measured CU
docs/proposals/               # sRFC, decision request, maintainer note, SIMD gate
scripts/                      # Optional Surfpool checklist
.upstream-token-2022-sha.txt  # Pinned upstream SHA for layout inspiration
```

## Program ID

Declared ID: `GyrTVV4hbcuzJuSz86FNq7K2UVAoSJQtcgHTVTz1hPPq`.  
Local `cargo build-sbf` may emit a different `target/deploy/*-keypair.json`; for Surfpool/deploy, use a keypair whose pubkey matches `declare_id!`, or regenerate both together. Never commit keypairs.

## Proposal path

1. [docs/SPEC.md](docs/SPEC.md) — normative reference behavior  
2. [docs/proposals/srfc-receive-policy-held-delivery.md](docs/proposals/srfc-receive-policy-held-delivery.md) — discussion draft  
3. [docs/proposals/decision-request.md](docs/proposals/decision-request.md) — mint flag / metas / greenfield / SIMD  
4. Maintainer discussion vs [#124](https://github.com/solana-program/token-2022/issues/124) (revert hook vs held delivery)  
5. SIMD **only if** maintainers confirm canonical Token-2022 / runtime scope  

License: [Apache-2.0](LICENSE). Program details: [program/token-2022-receive/README.md](program/token-2022-receive/README.md).
