/**
 * Cross-language wire contract.
 *
 * These byte vectors are asserted identically by the Rust side in
 * `program/token-2022-receive/tests/wire_vectors.rs`. If the two encoders ever disagree the
 * client silently builds instructions the program misreads, so both suites must be updated
 * together and neither may be changed alone.
 *
 *   node --experimental-strip-types --test src/index.test.ts
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  NO_HELD_DELIVERY,
  ORIGINATOR_RECOVERY_ONLY,
  UNLIMITED_HELD_LIMITS,
  decodeReceivePolicy,
  previewOutcome,
  decodeTransferOutcome,
  encodeInitializeReceivePolicy,
  encodeTransferChecked,
  encodeU64LE,
  transferCheckedAccounts,
  TransferOutcome,
} from "./index.ts";

const hex = (b: Uint8Array) =>
  Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");

test("encodeU64LE rejects values outside u64", () => {
  assert.equal(hex(encodeU64LE(1)), "0100000000000000");
  assert.equal(hex(encodeU64LE(2n ** 64n - 1n)), "ffffffffffffffff");
  assert.throws(() => encodeU64LE(-1));
  assert.throws(() => encodeU64LE(2n ** 64n));
});

test("TransferChecked wire vector", () => {
  const nonce = new Uint8Array(32).fill(9);
  const got = encodeTransferChecked({
    amount: 1n,
    decimals: 6,
    uniqueNonce: nonce,
    limits: UNLIMITED_HELD_LIMITS,
  });
  assert.equal(got.length, 59);
  assert.equal(
    hex(got),
    "04" +
      "0100000000000000" + // amount
      "06" + // decimals
      "09".repeat(32) + // uniqueNonce
      "ffffffffffffffff" + // maxBondLamports
      "ffffffffffffffff" + // maxTtlSlots
      "02", // maxRecoveryMode: ThirdParty, i.e. accept any
  );

  // Refusing held delivery outright.
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
  // Out-of-range recovery ceiling must fail before a transaction is paid for.
  assert.throws(() =>
    encodeTransferChecked({
      amount: 1n,
      decimals: 6,
      uniqueNonce: nonce,
      limits: { ...UNLIMITED_HELD_LIMITS, maxRecoveryMode: 3 },
    }),
  );
});

test("InitializeReceivePolicy wire vector", () => {
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
  assert.equal(
    hex(got),
    "02" +
      "6400000000000000" + // min_amount
      "0102" + // source_owner_mode, recovery_authority_mode
      "ab".repeat(32) +
      "0000000000000000" + // bond
      "4012170000000000" + // ttl 1_512_000 = 0x171240, little endian
      "00", // allowlist_len
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
  // A short key would shift every following field, silently corrupting bond and TTL.
  assert.throws(() => encodeInitializeReceivePolicy({ ...base, recoveryAuthority: short }));
  assert.throws(() => encodeInitializeReceivePolicy({ ...base, allowlist: [short] }));
  // Out-of-range modes are rejected on-chain; fail before spending a transaction on them.
  assert.throws(() => encodeInitializeReceivePolicy({ ...base, sourceOwnerMode: 7 }));
  assert.throws(() => encodeInitializeReceivePolicy({ ...base, recoveryAuthorityMode: 9 }));
  // Uint8Array.of coerces: -1 would become 255 and 1.5 would truncate to 1, i.e. a DIFFERENT
  // valid mode. An upper-bound check alone would let both through.
  assert.throws(() => encodeInitializeReceivePolicy({ ...base, sourceOwnerMode: -1 }));
  assert.throws(() => encodeInitializeReceivePolicy({ ...base, sourceOwnerMode: 1.5 }));
  assert.throws(() => encodeInitializeReceivePolicy({ ...base, recoveryAuthorityMode: NaN }));
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
      "02" + // allowlist_len
      "11".repeat(32) +
      "22".repeat(32),
  );
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
  // A wrong-length seed derives a different address in silence, which is worse than throwing.
  assert.throws(() => guardTokenSeeds(short, ok));
  assert.throws(() => guardTokenSeeds(ok, short));
  assert.throws(() => guardStateSeeds(short, ok));
  assert.throws(() => receiptSeeds(ok, ok, short, new Uint8Array(32)));
  assert.throws(() => receiptSeeds(ok, ok, ok, new Uint8Array(31)));
  assert.equal(guardTokenSeeds(ok, ok).length, 3);
  assert.equal(receiptSeeds(ok, ok, ok, new Uint8Array(32)).length, 5);
});

test("ReceivePolicy account layout vector, shared with wire_vectors.rs", () => {
  // Same bytes the Rust suite asserts. A sender needs the destination's terms to choose sensible
  // HeldLimits, and the policy is write-once, so what this reads cannot change under an
  // in-flight payment.
  const data = new Uint8Array(498);
  const put = (offset: number, h: string) => {
    for (let i = 0; i < h.length / 2; i++) data[offset + i] = parseInt(h.slice(i * 2, i * 2 + 2), 16);
  };
  put(
    165,
    "02" + // ACCOUNT_TYPE_ACCOUNT
      "1027" + // extension type 10_000
      "4801" + // declared length 328
      "6400000000000000" + // minAmount 100
      "01" + // sourceOwnerMode Allowlist
      "01" + // recoveryAuthorityMode Receiver
      "000000000000" +
      "ab".repeat(32) + // recoveryAuthority
      "0700000000000000" + // receiptBondLamports 7
      "4012170000000000", // receiptTtlSlots 1_512_000
  );
  data[234] = 1; // allowlistLen
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

  // A plain token account carries no policy; a corrupt one must throw rather than read as none.
  assert.equal(decodeReceivePolicy(new Uint8Array(165)), null);
  const corrupt = data.slice();
  corrupt[168] = 4; // declared length no longer matches the struct
  assert.throws(() => decodeReceivePolicy(corrupt));
});

test("previewOutcome tells a sender what will happen before paying", () => {
  const sender = new Uint8Array(32).fill(0x11);
  const stranger = new Uint8Array(32).fill(0x22);
  const policy = {
    minAmount: 100n,
    sourceOwnerMode: 1, // Allowlist
    recoveryAuthorityMode: 1, // Receiver claims what it rejects
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
  assert.equal(preview(50n, sender), "held"); // below minAmount
  assert.equal(preview(150n, stranger), "held"); // not on the allowlist
  // The sender refuses to hand recovery to the receiver, so the hold would fail on chain.
  assert.equal(preview(50n, sender, ORIGINATOR_RECOVERY_ONLY), "failed");
  assert.equal(preview(50n, sender, NO_HELD_DELIVERY), "failed");
  // A zero-amount hold is rejected on chain, so the preview must not say "held".
  assert.equal(preview(0n, sender), "failed");
  // No policy at all: an ordinary credit.
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
});

test("previewOutcome applies the rent floor to the bond", () => {
  // On chain the bond is max(policy.receiptBondLamports, rent(RECEIPT_SIZE)). A preview that
  // compares the raw policy field predicts "held" where the chain refuses the hold.
  const sender = new Uint8Array(32).fill(0x11);
  const policy = {
    minAmount: 100n,
    sourceOwnerMode: 0,
    recoveryAuthorityMode: 0,
    recoveryAuthority: new Uint8Array(32),
    receiptBondLamports: 0n, // asks for nothing, yet rent is still charged
    receiptTtlSlots: 1_000n,
    allowlist: [] as Uint8Array[],
  };
  const rent = 2_400_000n;
  const limits = { maxBondLamports: 1_000n, maxTtlSlots: 10_000n, maxRecoveryMode: 2 };

  // A zero rent floor is what the un-floored preview used to assume, and it is wrong.
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
  // With the real floor, the sender's 1_000 lamport ceiling cannot cover rent.
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
  // A ceiling above rent still holds.
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
