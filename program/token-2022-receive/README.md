# token-2022-receive

Token-2022-shaped **reference program** (custom program ID) with an opt-in destination **ReceivePolicy** extension and non-reverting **held delivery**.

Not a mint Transfer Hook. Held routing lives in transfer processing.  
Not canonical Token-2022. Does not intercept legacy USDC/USDT.

**Program ID:** `GyrTVV4hbcuzJuSz86FNq7K2UVAoSJQtcgHTVTz1hPPq`  
Upstream layout inspiration: [`solana-program/token-2022`](https://github.com/solana-program/token-2022) @ `5f64085`.  
Instruction tags are **reference-local** (not TokenzQd wire-compatible). Deploy keypair must match `declare_id!`.

## v0 defaults

| Parameter | Value |
| --- | --- |
| Open receipts per shard | unbounded; `GuardState.held_amount` backs custody |
| Default TTL | `1_512_000` slots |
| Allowlist cap | 8 pubkeys |
| Expiry | Return to source-owner same-mint account; refund bond |
| Bond | Explicit bond payer on held path |
| Mint allow-flag | Not required |
| Receipt PDA | `(receiver, mint, source_owner, unique_nonce)` |
| Transfer Hook | Unsupported in v0 |

## Outcomes

| Outcome | When |
| --- | --- |
| `failed` | Ordinary token failure, missing metas, or capacity |
| `credited` | Policy accepts → destination |
| `held` | Policy rejects → guard + receipt; `Ok` |

No-policy destinations keep the 4-account `TransferChecked` shape.

## Policy transfer accounts

1. source (w) 2. mint 3. destination (w) 4. authority (signer)  
5. guard_token (w) 6. guard_state (w) 7. receipt (w) 8. bond_payer (signer, w) 9. system_program  

Missing → `MissingPolicyAccounts` (never silent bypass).

## Build / test

```bash
export PATH="$HOME/.local/share/solana/install/active_release/bin:$PATH"
cargo test -p token-2022-receive
cargo build-sbf --manifest-path program/token-2022-receive/Cargo.toml
cargo test -p token-2022-receive --test litesvm_before_after -- --nocapture
```

Artifact: `target/deploy/token_2022_receive.so`. Evidence: [docs/VERIFICATION.md](../../docs/VERIFICATION.md). Semantics: [docs/SPEC.md](../../docs/SPEC.md).

## Unsupported / deferred

Full Token-2022 extension surface, Transfer Hook coexistence, Associated Token Program integration, full Kit/Codama SDK, mint allow-flag, confidential transfers.
