# @solana-receive/client

Kit / Codama client for the `token-2022-receive` reference program.

| Layer | What |
| --- | --- |
| `src/generated/` | **Codama-generated** instruction builders, PDAs, errors (`npm run codegen` from repo root) |
| `src/index.ts` residual | `previewOutcome`, HeldLimits presets, legacy byte encoders, policy TLV decode |

Not canonical Token-2022. Not TokenzQd wire-compatible. Not Tokenkeg USDC interception.

```bash
# From repo root: regenerate after IDL edits
npm install
npm run codegen

cd clients/js
npm install
npm run typecheck
npm test
```

Wire freeze: [`docs/WIRE.md`](../../docs/WIRE.md). IDL: [`idl/token-2022-receive.json`](../../idl/token-2022-receive.json).
