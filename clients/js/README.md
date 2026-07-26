# @solana-receive/client

Kit / Codama client for the `token-2022-receive` reference program.

| Layer | What |
| --- | --- |
| `src/generated/` | **Codama-generated** instruction builders, PDAs, errors (`npm run codegen` from repo root) |
| `src/*.ts` residual | `previewOutcome`, HeldLimits presets, policy TLV decode, validated encode wrappers |

## Non-claims

- Custom program ID (`GyrTVV4hbcuzJuSz86FNq7K2UVAoSJQtcgHTVTz1hPPq`)
- **Not** canonical Token-2022 / TokenzQd wire-compatible
- **Not** Tokenkeg USDC interception
- **Not** ambient ATA / wallet policy enforcement

## Consuming this package

It ships TypeScript sources with no build step: `main` and `exports` point at `.ts` files. That
needs a TypeScript-aware pipeline, which means a bundler or a type-stripping runtime such as
`node --experimental-strip-types`. A plain `tsc` consumer on `moduleResolution: nodenext` cannot
import `.ts` specifiers without `allowImportingTsExtensions`.

`npm run codegen` post-processes the Codama output so it stays erasable and explicitly
extensioned; `scripts/codegen-postprocess.mjs` fails the build if a codegen upgrade emits
something a stripping runtime cannot execute, and CI imports the entry point to prove it.

When calling `getTransferCheckedInstruction` directly, the five policy accounts (`guardToken`,
`guardState`, `receipt`, `bondPayer`, `systemProgram`) are positional and all-or-nothing: pass all
five for the held path or none for the 4-account path. A subset silently shifts the remainder into
the wrong slots.

## Install / regenerate

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

## Walkthrough: credited → held → claim / expiry

Use only this package. On-chain semantics: [`docs/SPEC.md`](../../docs/SPEC.md).

```ts
import {
  address,
  createNoopSigner, // or a real wallet signer
} from "@solana/kit";
import {
  TOKEN2022_RECEIVE_PROGRAM_ADDRESS,
  UNLIMITED_HELD_LIMITS,
  NO_HELD_DELIVERY,
  RECEIPT_SIZE,
  findGuardTokenPda,
  findGuardStatePda,
  findReceiptPdaChecked,
  getInitializeReceivePolicyInstruction,
  getEnsureGuardInstruction,
  getTransferCheckedInstruction,
  getClaimReceiptInstruction,
  getCloseExpiredReceiptInstruction,
  decodeReceivePolicy,
  previewOutcome,
  decodeTransferOutcome,
} from "@solana-receive/client";

// Addresses you already have (mint, ATAs, owners) — types are Kit `Address`.
declare const mint: ReturnType<typeof address>;
declare const destination: ReturnType<typeof address>;
declare const source: ReturnType<typeof address>;
declare const receiver: ReturnType<typeof address>;
declare const sourceOwner: ReturnType<typeof address>;
declare const owner: ReturnType<typeof createNoopSigner>;
declare const authority: ReturnType<typeof createNoopSigner>;
declare const payer: ReturnType<typeof createNoopSigner>;
declare const bondPayer: ReturnType<typeof createNoopSigner>;

// 1) Destination owner: write-once policy (once per account).
getInitializeReceivePolicyInstruction({
  tokenAccount: destination,
  owner,
  minAmount: 100n,
  sourceOwnerMode: 0, // AllowAll
  recoveryAuthorityMode: 0, // Originator claims rejected funds
  recoveryAuthority: sourceOwner,
  receiptBondLamports: 0n,
  receiptTtlSlots: 1_512_000n,
  allowlist: [],
});

// 2) Anyone: create/repair the (receiver, mint) guard shard before held delivery.
const [guardToken] = await findGuardTokenPda({ receiver, mint });
const [guardState] = await findGuardStatePda({ receiver, mint });
getEnsureGuardInstruction({
  payer,
  receiver,
  mint,
  guardToken,
  guardState,
  systemProgram: address("11111111111111111111111111111111"),
});

// 3) Sender: read terms, preview, then transfer.
const policy = decodeReceivePolicy(/* destination account data */);
const rentExemptReceiptLamports = /* connection.getMinimumBalanceForRentExemption(RECEIPT_SIZE) */;
const outcome = previewOutcome({
  policy,
  amount: 50n,
  sourceOwner: /* 32-byte pubkey */,
  limits: UNLIMITED_HELD_LIMITS, // or NO_HELD_DELIVERY to refuse holds
  rentExemptReceiptLamports,
});

const uniqueNonce = crypto.getRandomValues(new Uint8Array(32));
// uniqueNonce must be exactly 32 bytes (checked helper; generated findReceiptPda soft-pads).
const [receipt] = await findReceiptPdaChecked({
  receiver,
  mint,
  sourceOwner,
  uniqueNonce,
});

// No policy / self-transfer: omit guard* / receipt / bondPayer (4 accounts).
// Policy destination: pass all nine.
const ix = getTransferCheckedInstruction({
  source,
  mint,
  destination,
  authority,
  guardToken,
  guardState,
  receipt,
  bondPayer,
  systemProgram: address("11111111111111111111111111111111"),
  amount: 50n,
  decimals: 6,
  uniqueNonce: Array.from(uniqueNonce),
  maxBondLamports: UNLIMITED_HELD_LIMITS.maxBondLamports,
  maxTtlSlots: UNLIMITED_HELD_LIMITS.maxTtlSlots,
  maxRecoveryMode: UNLIMITED_HELD_LIMITS.maxRecoveryMode,
});

// After send: read return data (last ix in the tx).
decodeTransferOutcome(/* tx return data bytes */); // 0 credited, 1 held

// 4a) Held → claim (recovery authority signs).
getClaimReceiptInstruction({
  receipt,
  guardToken,
  guardState,
  claimDestination: /* same-mint token account */,
  mint,
  claimAuthority: authority,
  bondDest: bondPayer.address, // must be the recorded bond payer
});

// 4b) Or after TTL: anyone closes; tokens → source_owner token account; bond → bond_payer.
getCloseExpiredReceiptInstruction({
  receipt,
  guardToken,
  guardState,
  sourceOwnerToken: source,
  mint,
  bondDest: bondPayer.address,
});

void TOKEN2022_RECEIVE_PROGRAM_ADDRESS;
void outcome;
void ix;
void NO_HELD_DELIVERY;
void RECEIPT_SIZE;
```

Program ID in generated code: `TOKEN2022_RECEIVE_PROGRAM_ADDRESS`.
