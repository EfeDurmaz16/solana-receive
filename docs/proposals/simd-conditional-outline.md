# SIMD outline (conditional) — Destination Receive Policy / Held Delivery

> **Outline only — not ready to submit.**  
> Do not assign a SIMD number or open a SIMD PR until Token-2022 / runtime maintainers confirm the change belongs in canonical Token-2022 and/or runtime.  
> Working path until then: **sRFC + custom program ID reference**.  
> See [`srfc-receive-policy-held-delivery.md`](./srfc-receive-policy-held-delivery.md) and [`maintainer-discussion.md`](./maintainer-discussion.md).

## Gate checklist (all required)

- [ ] Maintainers agree the gap is not solved by Transfer Hook / Memo / #124-as-specified.
- [ ] Maintainers agree the layer is canonical Token-2022 (and/or name the runtime piece).
- [x] Reference program demonstrates `credited` and `held` without breaking no-policy transfers.
- [~] Differential vs upstream: **layout + no-policy amount overlap** in-tree (`upstream_differential`); **not** full `TokenzQd` execution parity — do not check this gate as done for SIMD.
- [x] Measured account count, transaction footprint, and CU ceilings published ([VERIFICATION](../VERIFICATION.md)).
- [x] Writable-lock structure documented and regression-checked (distinct shards share no writable guard account).
- [ ] Runtime contention / throughput measured under a real scheduler; LiteSVM does not model bank locks.
- [x] Wire surface frozen for clients ([WIRE](../WIRE.md)); Kit/Codama client in `clients/js`.
- [ ] Canonical open questions reduced (mint allow-flag, account-resolution shape) — see [decision-request.md](./decision-request.md).

**If any gate fails:** keep sRFC + custom program ID; do not file a SIMD.

## Proposed title (unassigned)

**SIMD-XXXX: Token-2022 Destination Account Receive Policy with Non-Reverting Held Delivery**

## Summary sketch

Opt-in account extension; policy reject → receiver-scoped guard + receipt (`held`), distinct from ordinary `failed` and policy-accept `credited`. Normative reference behavior: [`../SPEC.md`](../SPEC.md).

## Alternatives

| Alternative | Notes |
| --- | --- |
| App `deposit()` vault | Does not intercept ATA transfers |
| Mint Transfer Hook as receive policy | Wrong ownership; revert; no redirect |
| Ship only #124-style revert hook | Hard-stop, not held/recovery UX |
| Global per-mint guard | Writable contention |
| Research fork forever | Valid if maintainers decline canonical scope |

## Spec sections to expand only after gate

Extension fields; transfer processing outcomes; guard/receipt PDAs; client resolution; compatibility matrix; CU/tx budgets; security (fail-closed metas, bond, membership).

## References

- [`srfc-receive-policy-held-delivery.md`](./srfc-receive-policy-held-delivery.md)
- [`../SPEC.md`](../SPEC.md)
- [`../VERIFICATION.md`](../VERIFICATION.md)
- [token-2022#124](https://github.com/solana-program/token-2022/issues/124)
