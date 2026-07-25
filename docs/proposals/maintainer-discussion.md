# Maintainer discussion: #124 revert hook vs held delivery

**Audience:** Token-2022 maintainers  
**Related:** [token-2022#124](https://github.com/solana-program/token-2022/issues/124)  
**Draft:** [`srfc-receive-policy-held-delivery.md`](./srfc-receive-policy-held-delivery.md)  
**Decisions:** [`decision-request.md`](./decision-request.md)  
**Status:** Discussion starter — **not** a SIMD

---

## Short post (copy-paste)

**Title:** Receiver-side inbound policy: #124 revert hooks vs non-reverting held delivery

Hi Token / Token-2022 folks —

Design discussion adjacent to [#124 (account-side transfer hook)](https://github.com/solana-program/token-2022/issues/124).

**Problem.** Solana lacks a protocol-native combination of: (1) receiver-controlled policy on inbound delivery to a **destination token account**, (2) a **non-reverting** path when that policy rejects, and (3) attributable **recovery/claim** semantics. Mint Transfer Hooks are issuer-owned, typically revert, and receive transfer accounts read-only. App `deposit()` vaults are cooperative; ordinary ATA transfers never enter them.

**What #124 explores.** Per-account hook programs that **approve or reject**. On disapproval, the transfer **halts**. Mint allow-flag and tx-size notes still apply.

**What we propose (or alongside).** An opt-in destination-account receive-policy extension whose reject path is **held delivery**:

- Token-valid + policy reject → funds route **source → receiver-scoped guard**, receipt written, instruction **`Ok`**.
- Outcomes: `failed` \| `credited` \| `held`.
- Custody sharded by `(receiver, mint)` — no global guard.
- Receipts: bounded, bonded, TTL + permissionless close, full-claim only in v0.
- Membership evaluates **source token-account owner**.

Reference implementation: greenfield **custom program ID** (Token-2022-**shaped**, **not** wire-compatible with `TokenzQd` discriminators). Host + LiteSVM evidence and sRFC vectors are in the repo; see VERIFICATION.

We are **not** claiming legacy Tokenkeg USDC/USDT interception.

**Ask.**

1. Is this gap still unsolved at the Token-2022 layer?
2. Prefer first-class held-delivery in transfer processing, composition on a future #124 revert hook, or research-reference-only?
3. Should a mint allow-flag be required before accounts enable receive-policy? (reference default: **no**)
4. If canonical Token-2022 / runtime is in scope, we will draft a SIMD **after** that confirmation; until then: sRFC + custom program ID reference.

**One-liner:** #124 ≈ account-side **revert hook**; this proposal ≈ account-side **non-reverting held delivery**.

Decision checklist: [`decision-request.md`](./decision-request.md).

---

## Talking points

- Fail-closed metas: missing guard/receipt → `failed`, never silent bypass.
- Why not mint Transfer Hook: wrong ownership; read-only; revert-only; no fund redirect.
- Why not app vault as the answer: ATA path bypass.
- Contention: per-`(receiver, mint)` guard shards; same shard serializes; LiteSVM does not measure multi-tx bank locks.
- Path: sRFC first; SIMD conditional on canonical scope ([simd-conditional-outline.md](./simd-conditional-outline.md)).
