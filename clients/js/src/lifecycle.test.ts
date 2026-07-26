/**
 * Generated Kit builders for the credited → held → claim / expiry path.
 *
 * These tests assemble instructions from the public package API only (no hand-rolled
 * discriminators). On-chain execution stays in the Rust LiteSVM suites; here we pin account
 * roles, PDA derivation, and empty-body tags that a third-party client must get right.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  address,
  createNoopSigner,
  getAddressDecoder,
  getAddressEncoder,
  type Address,
} from "@solana/kit";

import {
  TOKEN2022_RECEIVE_PROGRAM_ADDRESS,
  UNLIMITED_HELD_LIMITS,
  encodeTransferChecked,
  findGuardStatePda,
  findGuardTokenPda,
  findReceiptPda,
  getClaimReceiptInstruction,
  getCloseExpiredReceiptInstruction,
  getEnsureGuardInstruction,
  getInitializeReceivePolicyInstruction,
  getTransferCheckedInstruction,
  previewOutcome,
  transferCheckedAccounts,
  RECEIPT_SIZE,
} from "./index.ts";

const pk = (fill: number): Address => getAddressDecoder().decode(new Uint8Array(32).fill(fill));

test("generated builders cover credited → held → claim / expiry assembly", async () => {
  const mint = pk(1);
  const receiver = pk(2);
  const sourceOwner = pk(3);
  const source = pk(4);
  const destination = pk(5);
  const claimDest = pk(6);
  const payer = createNoopSigner(pk(7));
  const authority = createNoopSigner(sourceOwner);
  const owner = createNoopSigner(receiver);
  const bondPayer = createNoopSigner(pk(8));

  const [guardToken] = await findGuardTokenPda({ receiver, mint });
  const [guardState] = await findGuardStatePda({ receiver, mint });
  const nonce = new Uint8Array(32).fill(9);
  const [receipt] = await findReceiptPda({
    receiver,
    mint,
    sourceOwner,
    uniqueNonce: nonce,
  });

  // 1. Write-once policy on the destination.
  const initPolicy = getInitializeReceivePolicyInstruction({
    tokenAccount: destination,
    owner,
    minAmount: 100n,
    sourceOwnerMode: 0,
    recoveryAuthorityMode: 0,
    recoveryAuthority: sourceOwner,
    receiptBondLamports: 0n,
    receiptTtlSlots: 1_512_000n,
    allowlist: [],
  });
  assert.equal(initPolicy.programAddress, TOKEN2022_RECEIVE_PROGRAM_ADDRESS);
  assert.equal(initPolicy.accounts.length, 2);
  assert.equal(initPolicy.data[0], 2);

  // 2. Ensure the guard shard exists before a held path can run.
  const ensure = getEnsureGuardInstruction({
    payer,
    receiver,
    mint,
    guardToken,
    guardState,
    systemProgram: address("11111111111111111111111111111111"),
  });
  assert.equal(ensure.accounts.length, 6);
  assert.equal(ensure.data[0], 3);

  // 3a. No-policy / credited path: 4 accounts.
  const credited = getTransferCheckedInstruction({
    source,
    mint,
    destination,
    authority,
    amount: 50n,
    decimals: 6,
    uniqueNonce: Array.from(nonce),
    maxBondLamports: UNLIMITED_HELD_LIMITS.maxBondLamports,
    maxTtlSlots: UNLIMITED_HELD_LIMITS.maxTtlSlots,
    maxRecoveryMode: UNLIMITED_HELD_LIMITS.maxRecoveryMode,
  });
  assert.equal(credited.accounts.length, 4);
  assert.equal(credited.data.length, 59);

  // 3b. Policy / held path: 9 accounts (same data layout).
  const held = getTransferCheckedInstruction({
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
    uniqueNonce: Array.from(nonce),
    maxBondLamports: UNLIMITED_HELD_LIMITS.maxBondLamports,
    maxTtlSlots: UNLIMITED_HELD_LIMITS.maxTtlSlots,
    maxRecoveryMode: UNLIMITED_HELD_LIMITS.maxRecoveryMode,
  });
  assert.equal(held.accounts.length, 9);
  assert.deepEqual(
    new Uint8Array(held.data),
    encodeTransferChecked({
      amount: 50n,
      decimals: 6,
      uniqueNonce: nonce,
      limits: UNLIMITED_HELD_LIMITS,
    }),
  );
  assert.deepEqual(
    transferCheckedAccounts({
      source,
      mint,
      destination,
      authority: sourceOwner,
      policy: {
        guardToken,
        guardState,
        receipt,
        bondPayer: bondPayer.address,
      },
    }).map((a) => a.address),
    held.accounts.map((a) => a.address),
  );

  // Preflight: below minAmount with unlimited limits → held (rent floor required).
  assert.equal(
    previewOutcome({
      policy: {
        minAmount: 100n,
        sourceOwnerMode: 0,
        recoveryAuthorityMode: 0,
        recoveryAuthority: new Uint8Array(32),
        receiptBondLamports: 0n,
        receiptTtlSlots: 1_512_000n,
        allowlist: [],
      },
      amount: 50n,
      sourceOwner: new Uint8Array(getAddressEncoder().encode(sourceOwner)),
      limits: UNLIMITED_HELD_LIMITS,
      rentExemptReceiptLamports: 2_400_000n,
    }),
    "held",
  );
  assert.equal(RECEIPT_SIZE, 304);

  // 4. Claim held funds.
  const claim = getClaimReceiptInstruction({
    receipt,
    guardToken,
    guardState,
    claimDestination: claimDest,
    mint,
    claimAuthority: authority,
    bondDest: bondPayer.address,
  });
  assert.equal(claim.accounts.length, 7);
  assert.equal(claim.data[0], 5);

  // 5. Or close after expiry (permissionless closer; bond still to recorded payer).
  const close = getCloseExpiredReceiptInstruction({
    receipt,
    guardToken,
    guardState,
    sourceOwnerToken: source,
    mint,
    bondDest: bondPayer.address,
  });
  assert.equal(close.accounts.length, 6);
  assert.equal(close.data[0], 6);
});

test("generated PDAs are stable for fixed seeds", async () => {
  const receiver = pk(0xaa);
  const mint = pk(0xbb);
  const sourceOwner = pk(0xcc);
  const nonce = new Uint8Array(32).fill(1);

  const [gt1] = await findGuardTokenPda({ receiver, mint });
  const [gt2] = await findGuardTokenPda({ receiver, mint });
  assert.equal(gt1, gt2);

  const [gs] = await findGuardStatePda({ receiver, mint });
  assert.notEqual(gt1, gs);

  const [r1] = await findReceiptPda({
    receiver,
    mint,
    sourceOwner,
    uniqueNonce: nonce,
  });
  const [r2] = await findReceiptPda({
    receiver,
    mint,
    sourceOwner,
    uniqueNonce: nonce,
  });
  assert.equal(r1, r2);
});
