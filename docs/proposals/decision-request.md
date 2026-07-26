# Decision request — receive-policy held delivery

**Audience:** Token-2022 / runtime maintainers (and anyone reviewing this reference)  
**Status:** One-page ask — not a SIMD  
**Related:** [srfc-receive-policy-held-delivery.md](./srfc-receive-policy-held-delivery.md), [maintainer-discussion.md](./maintainer-discussion.md), [../SPEC.md](../SPEC.md)  
**Repo:** https://github.com/EfeDurmaz16/solana-receive

This repository is a **custom program ID reference** inspired by Token-2022 layouts and transfer semantics. It is **not** a drop-in `TokenzQd…` binary and is **not wire-compatible** with upstream instruction discriminators (see `upstream_differential`).

Please decide the four items below. Until they are settled, the working path remains: **sRFC + this custom program ID**.

---

## 1. Mint allow-flag

| Option | Meaning |
| --- | --- |
| **A (reference default)** | Accounts may enable `ReceivePolicy` without a mint-level allow-flag |
| B | Mint must opt in before destinations can enable the extension |

**Reference choice:** A (SPEC §4).  
**Ask:** Prefer A, B, or “B only if canonical Token-2022”?

---

## 2. Account-resolution shape

Policy-enabled `TransferChecked` uses **9** accounts (4 base + guard_token, guard_state, receipt, bond_payer, system_program). Missing metas → `failed`. The five policy accounts are positional and all-or-nothing (see [WIRE.md](../WIRE.md)).

| Option | Meaning |
| --- | --- |
| **A (reference default)** | Keep this explicit 9-account shape |
| B | Prefer ExtraAccountMeta / resolution-account style if upstream adopts |

**Ask:** Is A acceptable for a first discussion, or must any canonical design start from B?

---

## 3. Custom reference vs upstream fork

| Option | Meaning |
| --- | --- |
| **A (this repo)** | Keep a Token-2022-**shaped** custom program ID reference |
| B | Rebase onto a true `solana-program/token-2022` fork aiming at upstream merge |
| C | Research-only; no path to canonical Token-2022 |

**Ask:** Which path should maintainers encourage?

---

## 4. Is a SIMD required?

| Option | Meaning |
| --- | --- |
| **A (default until confirmed)** | sRFC + reference program only; **no SIMD filed** |
| B | SIMD for Token-2022 (and/or runtime) after gates in [simd-conditional-outline.md](./simd-conditional-outline.md) |

**Ask:** Confirm whether held delivery belongs in canonical Token-2022 / runtime at all. If no, choose A/C and stop.

Ask 3 and Ask 4 are coupled: if maintainers encourage an upstream path in Ask 3,
Ask 4 decides whether that path needs a SIMD; if maintainers decline canonical
Token-2022 / runtime scope in Ask 4, Ask 3 collapses to custom reference or
research-only.

---

## Evidence already in-tree

| Kind | Where |
| --- | --- |
| Normative semantics | [`../SPEC.md`](../SPEC.md) |
| Frozen wire (tags, metas, PDAs, outcomes) | [`../WIRE.md`](../WIRE.md) |
| CU ceilings, footprints, LiteSVM | [`../VERIFICATION.md`](../VERIFICATION.md) |
| Golden / sRFC vectors | `cargo test -p token-2022-receive --test golden_vectors` |
| Layout overlap (not TokenzQd execution) | `--test upstream_differential` |
| Kit / Codama client | `clients/js` (generated builders + residual `previewOutcome`) |
| Surfpool RPC lifecycle | In-tree `./scripts/surfpool-lifecycle.sh`; manual/local evidence, not CI-gated |

**Non-claims:** not legacy Tokenkeg USDC/USDT interception; not ambient policy on unmodified Token-2022; not TokenzQd wire compatibility; not a performance guarantee from point CU samples; not a mainnet product.

## Response log (fill after posting)

| Date | Venue | Ask 1 | Ask 2 | Ask 3 | Ask 4 | Next step |
| --- | --- | --- | --- | --- | --- | --- |
| | | | | | | |
