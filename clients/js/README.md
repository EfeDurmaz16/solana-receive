# @solana-receive/helpers

Minimal TypeScript helpers for the `token-2022-receive` reference program:

- Program ID + v0 constants
- Instruction data encoders (`TransferChecked`, `InitializeReceivePolicy`)
- Account key lists (with/without policy metas)
- PDA seed helpers

Not a full Kit/Codama client.

```bash
cd clients/js && npm install && npm run typecheck
```
