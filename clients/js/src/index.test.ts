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
  const got = encodeTransferChecked({ amount: 1n, decimals: 6, uniqueNonce: nonce });
  assert.equal(got.length, 58);
  assert.equal(
    hex(got),
    "04" +
      "0100000000000000" + // amount
      "06" + // decimals
      "09".repeat(32) + // uniqueNonce
      "ffffffffffffffff" + // maxBondLamports, defaults to unlimited
      "ffffffffffffffff", // maxTtlSlots
  );

  // Refusing held delivery outright.
  const refused = encodeTransferChecked({
    amount: 1n,
    decimals: 6,
    uniqueNonce: nonce,
    limits: NO_HELD_DELIVERY,
  });
  assert.equal(hex(refused).slice(-32), "0".repeat(32));
  assert.throws(() =>
    encodeTransferChecked({ amount: 1n, decimals: 6, uniqueNonce: new Uint8Array(31) }),
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

test("decodeTransferOutcome distinguishes held from credited", () => {
  assert.equal(decodeTransferOutcome(new Uint8Array([0])), TransferOutcome.Credited);
  assert.equal(decodeTransferOutcome(new Uint8Array([1])), TransferOutcome.Held);
  assert.equal(decodeTransferOutcome(new Uint8Array()), null);
  assert.equal(decodeTransferOutcome(null), null);
  assert.throws(() => decodeTransferOutcome(new Uint8Array([2])));
});
