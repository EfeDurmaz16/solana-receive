# Decision request — receive-policy held delivery

**Audience:** Token-2022 / runtime maintainers (and anyone reviewing this reference)  
**Status:** One-page ask — not a SIMD  
**Related:** [srfc-receive-policy-held-delivery.md](./srfc-receive-policy-held-delivery.md), [maintainer-discussion.md](./maintainer-discussion.md), [../SPEC.md](../SPEC.md)

This repository is a **greenfield custom program ID reference** inspired by Token-2022 layouts and transfer semantics. It is **not** a drop-in `TokenzQd…` binary and is **not wire-compatible** with upstream instruction discriminators (see `upstream_differential`).

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

Policy-enabled `TransferChecked` uses **9** accounts (4 base + guard_token, guard_state, receipt, bond_payer, system_program). Missing metas → `failed`.

| Option | Meaning |
| --- | --- |
| **A (reference default)** | Keep this explicit 9-account shape |
| B | Prefer ExtraAccountMeta / resolution-account style if upstream adopts |

**Ask:** Is A acceptable for a first discussion, or must any canonical design start from B?

---

## 3. Greenfield reference vs upstream fork

| Option | Meaning |
| --- | --- |
| **A (this repo)** | Keep a greenfield / Token-2022-**shaped** custom program ID as the scientific deliverable |
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

---

## Evidence already in-tree

- Normative: [`../SPEC.md`](../SPEC.md)  
- Measured CU / ceilings / footprints: [`../VERIFICATION.md`](../VERIFICATION.md)  
- Executable vectors: `cargo test -p token-2022-receive --test golden_vectors`  
- Layout overlap (not TokenzQd execution): `--test upstream_differential`  

**Non-claims:** not legacy Tokenkeg USDC/USDT interception; not ambient policy on unmodified Token-2022; not a performance guarantee from point CU samples.
