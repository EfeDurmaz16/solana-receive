/**
 * Cross-language wire contract + residual helpers.
 *
 * Byte vectors match `program/token-2022-receive/tests/wire_vectors.rs`. Encoders pack via
 * Codama; residual wrappers only add fail-closed input checks.
 *
 *   npm test
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  DEFAULT_RECEIPT_TTL_SLOTS,
  MAX_RECEIPT_BOND_LAMPORTS,
  MAX_RECEIPT_TTL_SLOTS,
  NO_HELD_DELIVERY,
  ORIGINATOR_RECOVERY_ONLY,
  UNLIMITED_HELD_LIMITS,
  decodeReceivePolicy,
  previewOutcome,
  decodeTransferOutcome,
  encodeInitializeReceivePolicy,
  encodeTransferChecked,
  encodeEnsureGuard,
  encodeClaimReceipt,
  encodeCloseExpiredReceipt,
  transferCheckedAccounts,
  TransferOutcome,
  Ix,
  CLAIM_RECEIPT_DISCRIMINATOR,
  CLOSE_EXPIRED_RECEIPT_DISCRIMINATOR,
  ENSURE_GUARD_DISCRIMINATOR,
  TRANSFER_CHECKED_DISCRIMINATOR,
  INITIALIZE_RECEIVE_POLICY_DISCRIMINATOR,
} from "./index.ts";

const hex = (b: Uint8Array) =>
  Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");

const U64_OVERFLOW = 1n << 64n;

test("TransferChecked wire vector (Codama-backed)", () => {
  const nonce = new Uint8Array(32).fill(9);
  const got = encodeTransferChecked({
    amount: 1n,
    decimals: 6,
    uniqueNonce: nonce,
    limits: UNLIMITED_HELD_LIMITS,
  });
  assert.equal(got.length, 59);
  assert.equal(got[0], TRANSFER_CHECKED_DISCRIMINATOR);
  assert.equal(got[0], Ix.TransferChecked);
  assert.equal(
    hex(got),
    "04" +
      "0100000000000000" +
      "06" +
      "09".repeat(32) +
      "ffffffffffffffff" +
      "ffffffffffffffff" +
      "02",
  );

  const refused = encodeTransferChecked({
    amount: 1n,
    decimals: 6,
    uniqueNonce: nonce,
    limits: NO_HELD_DELIVERY,
  });
  assert.equal(hex(refused).slice(-34), "0".repeat(34));
  assert.throws(() =>
    encodeTransferChecked({
      amount: 1n,
      decimals: 6,
      uniqueNonce: new Uint8Array(31),
      limits: UNLIMITED_HELD_LIMITS,
    }),
  );
  assert.throws(() =>
    encodeTransferChecked({
      amount: 1n,
      decimals: 6,
      uniqueNonce: nonce,
      limits: { ...UNLIMITED_HELD_LIMITS, maxRecoveryMode: 3 },
    }),
  );
  assert.throws(() =>
    encodeTransferChecked({
      amount: Number.MAX_SAFE_INTEGER + 1,
      decimals: 6,
      uniqueNonce: nonce,
      limits: UNLIMITED_HELD_LIMITS,
    }),
  );
  assert.throws(() =>
    encodeTransferChecked({
      amount: U64_OVERFLOW,
      decimals: 6,
      uniqueNonce: nonce,
      limits: UNLIMITED_HELD_LIMITS,
    }),
  );
  assert.throws(() =>
    encodeTransferChecked({
      amount: 1n,
      decimals: 6,
      uniqueNonce: nonce,
      limits: { ...UNLIMITED_HELD_LIMITS, maxTtlSlots: Number.MAX_SAFE_INTEGER + 1 },
    }),
  );
});

test("InitializeReceivePolicy wire vector (Codama-backed)", () => {
  const authority = new Uint8Array(32).fill(0xab);
  const got = encodeInitializeReceivePolicy({
    minAmount: 100n,
    sourceOwnerMode: 1,
    recoveryAuthorityMode: 2,
    recoveryAuthority: authority,
    receiptBondLamports: 0n,
    receiptTtlSlots: 1_512_000n,
    allowlist: [],
  });
  assert.equal(got[0], INITIALIZE_RECEIVE_POLICY_DISCRIMINATOR);
  assert.equal(
    hex(got),
    "02" +
      "6400000000000000" +
      "0102" +
      "ab".repeat(32) +
      "0000000000000000" +
      "4012170000000000" +
      "00",
  );
});

test("policy encoder rejects wrong-length keys and out-of-range modes", () => {
  const short = new Uint8Array(31);
  const ok = new Uint8Array(32);
  const base = {
    minAmount: 0n,
    sourceOwnerMode: 0,
    recoveryAuthorityMode: 0,
    recoveryAuthority: ok,
    receiptBondLamports: 0n,
    receiptTtlSlots: 0n,
    allowlist: [] as Uint8Array[],
  };
  assert.throws(() => encodeInitializeReceivePolicy({ ...base, recoveryAuthority: short }));
  assert.throws(() => encodeInitializeReceivePolicy({ ...base, allowlist: [short] }));
  assert.throws(() => encodeInitializeReceivePolicy({ ...base, sourceOwnerMode: 7 }));
  assert.throws(() => encodeInitializeReceivePolicy({ ...base, recoveryAuthorityMode: 9 }));
  assert.throws(() => encodeInitializeReceivePolicy({ ...base, sourceOwnerMode: -1 }));
  assert.throws(() => encodeInitializeReceivePolicy({ ...base, sourceOwnerMode: 1.5 }));
  assert.throws(() => encodeInitializeReceivePolicy({ ...base, recoveryAuthorityMode: NaN }));
});

test("policy encoder rejects unsafe u64s and protocol cap overflows", () => {
  const ok = new Uint8Array(32);
  const base = {
    minAmount: 0n,
    sourceOwnerMode: 0,
    recoveryAuthorityMode: 0,
    recoveryAuthority: ok,
    receiptBondLamports: 0n,
    receiptTtlSlots: 0n,
    allowlist: [] as Uint8Array[],
  };
  assert.throws(() =>
    encodeInitializeReceivePolicy({
      ...base,
      minAmount: Number.MAX_SAFE_INTEGER + 1,
    }),
  );
  assert.throws(() =>
    encodeInitializeReceivePolicy({
      ...base,
      receiptBondLamports: BigInt(MAX_RECEIPT_BOND_LAMPORTS) + 1n,
    }),
  );
  assert.throws(() =>
    encodeInitializeReceivePolicy({
      ...base,
      receiptTtlSlots: BigInt(MAX_RECEIPT_TTL_SLOTS) + 1n,
    }),
  );
  assert.doesNotThrow(() =>
    encodeInitializeReceivePolicy({
      ...base,
      // Zero is the explicit on-wire "use default" sentinel; validate the default, encode zero.
      receiptTtlSlots: 0n,
    }),
  );
  assert.ok(DEFAULT_RECEIPT_TTL_SLOTS <= MAX_RECEIPT_TTL_SLOTS);
});

test("allowlist wire vector pins the variable-length field", () => {
  const a = new Uint8Array(32).fill(0x11);
  const b = new Uint8Array(32).fill(0x22);
  const got = encodeInitializeReceivePolicy({
    minAmount: 0n,
    sourceOwnerMode: 1,
    recoveryAuthorityMode: 0,
    recoveryAuthority: new Uint8Array(32),
    receiptBondLamports: 0n,
    receiptTtlSlots: 0n,
    allowlist: [a, b],
  });
  assert.equal(
    hex(got),
    "02" +
      "0000000000000000" +
      "0100" +
      "00".repeat(32) +
      "0000000000000000" +
      "0000000000000000" +
      "02" +
      "11".repeat(32) +
      "22".repeat(32),
  );
});

test("empty-body lifecycle instructions are a single tag byte", () => {
  assert.deepEqual(encodeEnsureGuard(), Uint8Array.of(ENSURE_GUARD_DISCRIMINATOR));
  assert.deepEqual(encodeClaimReceipt(), Uint8Array.of(CLAIM_RECEIPT_DISCRIMINATOR));
  assert.deepEqual(
    encodeCloseExpiredReceipt(),
    Uint8Array.of(CLOSE_EXPIRED_RECEIPT_DISCRIMINATOR),
  );
  assert.equal(ENSURE_GUARD_DISCRIMINATOR, Ix.EnsureGuard);
  assert.equal(CLAIM_RECEIPT_DISCRIMINATOR, Ix.ClaimReceipt);
  assert.equal(CLOSE_EXPIRED_RECEIPT_DISCRIMINATOR, Ix.CloseExpiredReceipt);
});

test("transferCheckedAccounts marks signers and writables", () => {
  const p = (s: string) => s;
  const withPolicy = transferCheckedAccounts({
    source: p("src"),
    mint: p("mint"),
    destination: p("dst"),
    authority: p("auth"),
    policy: {
      guardToken: p("gt"),
      guardState: p("gs"),
      receipt: p("r"),
      bondPayer: p("bp"),
    },
  });
  assert.equal(withPolicy.length, 9);
  assert.deepEqual(
    withPolicy.map((a) => `${a.isSigner ? "s" : "-"}${a.isWritable ? "w" : "-"}`),
    ["-w", "--", "-w", "s-", "-w", "-w", "-w", "sw", "--"],
  );
  assert.equal(
    transferCheckedAccounts({
      source: p("src"),
      mint: p("mint"),
      destination: p("dst"),
      authority: p("auth"),
    }).length,
    4,
  );
});

test("PDA seed builders reject wrong-length keys", async () => {
  const { guardTokenSeeds, guardStateSeeds, receiptSeeds } = await import("./constants.ts");
  const ok = new Uint8Array(32);
  const short = new Uint8Array(31);
  assert.throws(() => guardTokenSeeds(short, ok));
  assert.throws(() => guardTokenSeeds(ok, short));
  assert.throws(() => guardStateSeeds(short, ok));
  assert.throws(() => receiptSeeds(ok, ok, short, new Uint8Array(32)));
  assert.throws(() => receiptSeeds(ok, ok, ok, new Uint8Array(31)));
  assert.equal(guardTokenSeeds(ok, ok).length, 3);
  assert.equal(receiptSeeds(ok, ok, ok, new Uint8Array(32)).length, 5);
});

test("ReceivePolicy account layout vector, shared with wire_vectors.rs", () => {
  const data = new Uint8Array(498);
  const put = (offset: number, h: string) => {
    for (let i = 0; i < h.length / 2; i++) data[offset + i] = parseInt(h.slice(i * 2, i * 2 + 2), 16);
  };
  put(
    165,
    "02" +
      "1027" +
      "4801" +
      "6400000000000000" +
      "01" +
      "01" +
      "000000000000" +
      "ab".repeat(32) +
      "0700000000000000" +
      "4012170000000000",
  );
  data[234] = 1;
  put(242, "11".repeat(32));

  const policy = decodeReceivePolicy(data);
  assert.ok(policy);
  assert.equal(policy.minAmount, 100n);
  assert.equal(policy.sourceOwnerMode, 1);
  assert.equal(policy.recoveryAuthorityMode, 1);
  assert.equal(hex(policy.recoveryAuthority), "ab".repeat(32));
  assert.equal(policy.receiptBondLamports, 7n);
  assert.equal(policy.receiptTtlSlots, 1_512_000n);
  assert.equal(policy.allowlist.length, 1);
  assert.equal(hex(policy.allowlist[0]!), "11".repeat(32));

  assert.equal(decodeReceivePolicy(new Uint8Array(165)), null);
  const corrupt = data.slice();
  corrupt[168] = 4;
  assert.throws(() => decodeReceivePolicy(corrupt));
});

test("decodeReceivePolicy rejects foreign TLVs before, after, and without policy", () => {
  const put = (data: Uint8Array, offset: number, h: string) => {
    for (let i = 0; i < h.length / 2; i++) {
      data[offset + i] = parseInt(h.slice(i * 2, i * 2 + 2), 16);
    }
  };
  const writePolicy = (data: Uint8Array, offset: number) => {
    put(
      data,
      offset,
      "1027" +
        "4801" +
        "0000000000000000" +
        "00" +
        "00" +
        "000000000000" +
        "00".repeat(32) +
        "0000000000000000" +
        "4012170000000000" +
        "00" +
        "00000000000000" +
        "00".repeat(8 * 32),
    );
  };
  const writeForeign = (data: Uint8Array, offset: number) => {
    data[offset] = 0x2a;
    data[offset + 1] = 0x00;
    data[offset + 2] = 0x00;
    data[offset + 3] = 0x00;
  };

  const policyOnly = new Uint8Array(498);
  policyOnly[165] = 2;
  writePolicy(policyOnly, 166);
  assert.ok(decodeReceivePolicy(policyOnly));

  const foreignBefore = new Uint8Array(502);
  foreignBefore[165] = 2;
  writeForeign(foreignBefore, 166);
  writePolicy(foreignBefore, 170);
  assert.throws(() => decodeReceivePolicy(foreignBefore), /unsupported account extension type/);

  const foreignAfter = new Uint8Array(502);
  foreignAfter[165] = 2;
  writePolicy(foreignAfter, 166);
  writeForeign(foreignAfter, 498);
  assert.throws(() => decodeReceivePolicy(foreignAfter), /unsupported account extension type/);

  const foreignOnly = new Uint8Array(170);
  foreignOnly[165] = 2;
  writeForeign(foreignOnly, 166);
  assert.throws(() => decodeReceivePolicy(foreignOnly), /unsupported account extension type/);
});

test("previewOutcome tells a sender what will happen before paying", () => {
  const sender = new Uint8Array(32).fill(0x11);
  const stranger = new Uint8Array(32).fill(0x22);
  const policy = {
    minAmount: 100n,
    sourceOwnerMode: 1,
    recoveryAuthorityMode: 1,
    recoveryAuthority: new Uint8Array(32),
    receiptBondLamports: 7n,
    receiptTtlSlots: 1_512_000n,
    allowlist: [sender],
  };
  const rent = 2_400_000n;
  const preview = (amount: bigint, who: Uint8Array, limits = UNLIMITED_HELD_LIMITS) =>
    previewOutcome({
      policy,
      amount,
      sourceOwner: who,
      limits,
      rentExemptReceiptLamports: rent,
    });

  assert.equal(preview(150n, sender), "credited");
  assert.equal(preview(50n, sender), "held");
  assert.equal(preview(150n, stranger), "held");
  assert.equal(preview(50n, sender, ORIGINATOR_RECOVERY_ONLY), "failed");
  assert.equal(preview(50n, sender, NO_HELD_DELIVERY), "failed");
  assert.equal(preview(0n, sender), "failed");
  assert.equal(
    previewOutcome({
      policy: null,
      amount: 1n,
      sourceOwner: sender,
      limits: NO_HELD_DELIVERY,
      rentExemptReceiptLamports: rent,
    }),
    "credited",
  );
  assert.equal(preview(50n, sender, { ...UNLIMITED_HELD_LIMITS, maxBondLamports: 1n }), "failed");
  assert.throws(() =>
    previewOutcome({
      policy,
      amount: Number.MAX_SAFE_INTEGER + 1,
      sourceOwner: sender,
      limits: UNLIMITED_HELD_LIMITS,
      rentExemptReceiptLamports: rent,
    }),
  );
});

test("previewOutcome fails closed on unknown policy modes", () => {
  const sender = new Uint8Array(32).fill(0x11);
  const base = {
    minAmount: 100n,
    sourceOwnerMode: 1,
    recoveryAuthorityMode: 0,
    recoveryAuthority: new Uint8Array(32),
    receiptBondLamports: 7n,
    receiptTtlSlots: 1_512_000n,
    allowlist: [sender],
  };
  assert.equal(
    previewOutcome({
      policy: { ...base, sourceOwnerMode: 2 },
      amount: 150n,
      sourceOwner: sender,
      limits: UNLIMITED_HELD_LIMITS,
      rentExemptReceiptLamports: 2_400_000n,
    }),
    "failed",
  );
  assert.equal(
    previewOutcome({
      policy: { ...base, recoveryAuthorityMode: 3 },
      amount: 50n,
      sourceOwner: sender,
      limits: UNLIMITED_HELD_LIMITS,
      rentExemptReceiptLamports: 2_400_000n,
    }),
    "failed",
  );
});

test("previewOutcome applies the rent floor to the bond", () => {
  const sender = new Uint8Array(32).fill(0x11);
  const policy = {
    minAmount: 100n,
    sourceOwnerMode: 0,
    recoveryAuthorityMode: 0,
    recoveryAuthority: new Uint8Array(32),
    receiptBondLamports: 0n,
    receiptTtlSlots: 1_000n,
    allowlist: [] as Uint8Array[],
  };
  const rent = 2_400_000n;
  const limits = { maxBondLamports: 1_000n, maxTtlSlots: 10_000n, maxRecoveryMode: 2 };

  assert.equal(
    previewOutcome({
      policy,
      amount: 1n,
      sourceOwner: sender,
      limits,
      rentExemptReceiptLamports: 0n,
    }),
    "held",
  );
  assert.equal(
    previewOutcome({
      policy,
      amount: 1n,
      sourceOwner: sender,
      limits,
      rentExemptReceiptLamports: rent,
    }),
    "failed",
  );
  assert.equal(
    previewOutcome({
      policy,
      amount: 1n,
      sourceOwner: sender,
      limits: { ...limits, maxBondLamports: rent },
      rentExemptReceiptLamports: rent,
    }),
    "held",
  );
});

test("decodeTransferOutcome distinguishes held from credited", () => {
  assert.equal(decodeTransferOutcome(new Uint8Array([0])), TransferOutcome.Credited);
  assert.equal(decodeTransferOutcome(new Uint8Array([1])), TransferOutcome.Held);
  assert.equal(decodeTransferOutcome(new Uint8Array()), null);
  assert.equal(decodeTransferOutcome(null), null);
  assert.throws(() => decodeTransferOutcome(new Uint8Array([2])));
});

test("remaining tag wire vectors, shared with wire_vectors.rs", async () => {
  // Same bytes the Rust suite asserts. InitializeMint2's Option<Pubkey> is the one field where
  // the IDL can silently produce a body the program rejects: it must be a u8 prefix with NO
  // payload when None. A fixed-size option would append 32 zero bytes, and `unpack` rejects the
  // remainder as trailing bytes.
  const {
    getInitializeMint2InstructionDataEncoder,
    getInitializeAccount3InstructionDataEncoder,
    getMintToInstructionDataEncoder,
  } = await import("./generated/instructions/index.ts");

  const { getAddressDecoder } = await import("@solana/kit");
  const addr = (b: Uint8Array) => getAddressDecoder().decode(b);
  const authority = new Uint8Array(32).fill(0xcd);
  const freeze = new Uint8Array(32).fill(0xef);

  const none = new Uint8Array(
    getInitializeMint2InstructionDataEncoder().encode({
      decimals: 6,
      mintAuthority: addr(authority),
      freezeAuthority: null,
    }),
  );
  assert.equal(none.length, 35);
  assert.equal(hex(none), "00" + "06" + "cd".repeat(32) + "00");

  const some = new Uint8Array(
    getInitializeMint2InstructionDataEncoder().encode({
      decimals: 6,
      mintAuthority: addr(authority),
      freezeAuthority: addr(freeze),
    }),
  );
  assert.equal(some.length, 67);
  assert.equal(hex(some), "00" + "06" + "cd".repeat(32) + "01" + "ef".repeat(32));

  const initAccount = new Uint8Array(
    getInitializeAccount3InstructionDataEncoder().encode({
      owner: addr(new Uint8Array(32).fill(0x11)),
    }),
  );
  assert.equal(initAccount.length, 33);
  assert.equal(hex(initAccount), "01" + "11".repeat(32));

  const mintTo = new Uint8Array(
    getMintToInstructionDataEncoder().encode({ amount: 7n }),
  );
  assert.equal(mintTo.length, 9);
  assert.equal(hex(mintTo), "07" + "0700000000000000");
});
