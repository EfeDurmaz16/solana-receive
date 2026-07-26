# Maintainer discussion: #124 revert hook vs held delivery

**Audience:** Token-2022 maintainers  
**Related:** [token-2022#124](https://github.com/solana-program/token-2022/issues/124)  
**Repo:** https://github.com/EfeDurmaz16/solana-receive  
**Draft:** [`srfc-receive-policy-held-delivery.md`](./srfc-receive-policy-held-delivery.md)  
**Decisions:** [`decision-request.md`](./decision-request.md)  
**Status:** Superseded by [`srfc-43-receive-terms.md`](./srfc-43-receive-terms.md).

This document argued for held delivery as a change to canonical Token-2022, addressed to the
maintainers on #124. That framing was set aside: the work is now an application standard on
unmodified Token-2022, which needs no core change. Two arguments made below are withdrawn in the
successor and should not be reused.

1. "App `deposit()` vaults are cooperative; ordinary ATA transfers never enter them." Equally true
   of the reference program, whose accounts cannot be ATAs at all and whose policy destinations
   require a sender to pass extra accounts and sign a bond. The real differentiator is custody
   under a program-derived address, not reach.
2. "Chain return data / logs are authoritative for held vs credited." They are not. Return data is
   transaction-scoped and can be overwritten by a later instruction; logs can be truncated by
   padding the transaction or forged by any program in it. The authoritative signal is the
   destination token account's balance delta.

Kept for provenance.

## Short post (retained for provenance)

**Title:** Receiver-side inbound policy: #124 revert hooks vs non-reverting held delivery

Hi Token / Token-2022 folks —

Design discussion adjacent to [#124 (account-side transfer hook)](https://github.com/solana-program/token-2022/issues/124).

**Problem.** Solana lacks a protocol-native combination of: (1) receiver-controlled policy on inbound delivery to a **destination token account**, (2) a **non-reverting** path when that policy rejects, and (3) attributable **recovery/claim** semantics. Mint Transfer Hooks are issuer-owned, typically revert, and receive transfer accounts read-only. App `deposit()` vaults are cooperative; ordinary ATA transfers never enter them.

**What #124 explores.** Per-account hook programs that **approve or reject**. On disapproval, the transfer **halts**. Mint allow-flag and tx-size notes still apply.

**What we propose (or alongside).** An opt-in destination-account receive-policy whose reject path is **held delivery**:

- Token-valid + policy reject → funds route **source → receiver-scoped guard**, receipt written, instruction **`Ok`**.
- Outcomes: `failed` | `credited` | `held`.
- Custody sharded by `(receiver, mint)` — no global guard.
- Receipts: bounded, bonded, TTL + permissionless close, full-claim only in v0.
- Membership evaluates **source token-account owner**.

**Reference (custom program ID).** Token-2022-**shaped**, **not** wire-compatible with `TokenzQd` discriminators. In the repo today:

- Normative behavior: `docs/SPEC.md`
- Frozen wire for clients: `docs/WIRE.md`
- Host + LiteSVM suites, golden vectors, CU ceilings: `docs/VERIFICATION.md`
- Codama → Kit JS client (`clients/js`) for third-party assembly
- Surfpool offline RPC lifecycle (credited → held → claim / expiry) via `./scripts/surfpool-lifecycle.sh`

Repo: https://github.com/EfeDurmaz16/solana-receive

We are **not** claiming legacy Tokenkeg USDC/USDT interception, ambient policy on unmodified Token-2022, or a mainnet product.

**Ask.**

1. Mint allow-flag: allow account-level `ReceivePolicy` without a mint flag, require a mint flag, or require one only for a canonical Token-2022 path?
2. Account resolution: is the explicit 9-account policy transfer shape acceptable for discussion, or should any canonical path start from ExtraAccountMeta / resolution accounts?
3. Reference path: keep this custom program ID reference, rebase toward an upstream Token-2022 fork, or treat the work as research-only?
4. SIMD: does held delivery belong in canonical Token-2022 / runtime at all? If not, we keep sRFC + custom program ID and stop there.

**One-liner:** #124 ≈ account-side **revert hook**; this proposal ≈ account-side **non-reverting held delivery**.

Decision checklist: `docs/proposals/decision-request.md` in the repo.

---

## Talking points

- Fail-closed metas: missing guard/receipt → `failed`, never silent bypass.
- Why not mint Transfer Hook: wrong ownership; read-only; revert-only; no fund redirect.
- Why not app vault as the answer: ATA path bypass.
- CU (LiteSVM, not Surfpool): no-policy ~3k; policy credited ~7–16k; held ~13–22k; ceilings 40–50k. Held costs more (receipt + guard + PDA bump search); still under default budget. Point samples are not a performance claim.
- Contention: per-`(receiver, mint)` guard shards; same shard serializes; LiteSVM does not measure multi-tx bank locks.
- Client path: IDL → Codama → Kit; `previewOutcome` is advisory; chain return data / logs are authoritative for held vs credited.
- Path: sRFC first; SIMD conditional on canonical scope ([simd-conditional-outline.md](./simd-conditional-outline.md)).

## After responses

Record answers against the four asks in [decision-request.md](./decision-request.md). Then choose: stay-custom / compose-on-#124 / rebase-toward-upstream / stop. Do not open a SIMD unless maintainers confirm canonical scope.
