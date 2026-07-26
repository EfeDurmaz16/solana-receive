/**
 * Client for the token-2022-receive reference program.
 *
 * Prefer Codama-generated Kit builders (`./generated`, re-exported below). Residual helpers
 * cover preflight (`previewOutcome`), policy TLV decode, HeldLimits presets, and thin
 * validated wrappers that refuse bad inputs before they hit the wire.
 */

import {
  getAddressDecoder,
  type Address,
  type ProgramDerivedAddress,
} from "@solana/kit";
import {
  getClaimReceiptInstructionDataEncoder,
  getCloseExpiredReceiptInstructionDataEncoder,
  getEnsureGuardInstructionDataEncoder,
  getInitializeReceivePolicyInstructionDataEncoder,
  getTransferCheckedInstructionDataEncoder,
} from "./generated/instructions/index.ts";
import {
  findReceiptPda as findReceiptPdaGenerated,
  type ReceiptSeeds,
} from "./generated/pdas/receipt.ts";

/** Codama-generated Kit builders, PDAs, and typed errors. */
export * from "./generated/index.ts";

export {
  PROGRAM_ID,
  DEFAULT_RECEIPT_TTL_SLOTS,
  MAX_RECEIPT_TTL_SLOTS,
  MAX_RECEIPT_BOND_LAMPORTS,
  ALLOWLIST_CAP,
  SEEDS,
  guardTokenSeeds,
  guardStateSeeds,
  receiptSeeds,
} from "./constants.ts";

import {
  ALLOWLIST_CAP,
  DEFAULT_RECEIPT_TTL_SLOTS,
  MAX_RECEIPT_BOND_LAMPORTS,
  MAX_RECEIPT_TTL_SLOTS,
} from "./constants.ts";

export {
  deriveGuardTokenAddress,
  deriveGuardStateAddress,
  deriveReceiptAddress,
} from "./pda.ts";
export type { AddressApi } from "./pda.ts";

/** Instruction tags — same values as generated `*_DISCRIMINATOR` constants. */
export const Ix = {
  InitializeMint2: 0,
  InitializeAccount3: 1,
  InitializeReceivePolicy: 2,
  EnsureGuard: 3,
  TransferChecked: 4,
  ClaimReceipt: 5,
  CloseExpiredReceipt: 6,
  MintTo: 7,
} as const;
export type Ix = (typeof Ix)[keyof typeof Ix];

export type PolicyTransferAccounts = {
  guardToken: string;
  guardState: string;
  receipt: string;
  bondPayer: string;
};

/** Pubkeys are fixed-width on the wire; a short one shifts every field after it. */
function requirePubkey(bytes: Uint8Array, label: string): Uint8Array {
  if (bytes.length !== 32) {
    throw new Error(`${label} must be 32 bytes, got ${bytes.length}`);
  }
  return bytes;
}

function addressFromBytes(bytes: Uint8Array, label: string): Address {
  return getAddressDecoder().decode(requirePubkey(bytes, label));
}

/**
 * Single-byte fields must be validated on both sides.
 *
 * `Uint8Array.of` applies ToUint8: -1 becomes 255 and 1.5 truncates to 1, so an upper-bound
 * check alone lets a wrong value through as a different, valid on-chain mode.
 */
function requireByte(value: number, max: number, label: string): number {
  if (!Number.isInteger(value) || value < 0 || value > max) {
    throw new RangeError(`${label} must be an integer in [0, ${max}], got ${value}`);
  }
  return value;
}

/**
 * Sender-declared ceilings on a held outcome.
 *
 * The destination writes the policy but the sender pays for it: the bond is debited from
 * `bondPayer` and the TTL decides how long a rejected transfer stays locked. Omit for
 * `UNLIMITED_HELD_LIMITS`; use `NO_HELD_DELIVERY` to make a policy rejection fail rather than
 * lock funds.
 */
export type HeldLimits = {
  maxBondLamports: bigint | number;
  maxTtlSlots: bigint | number;
  /** 0 Originator, 1 Receiver, 2 ThirdParty. Bounds custody, not just cost. */
  maxRecoveryMode: number;
};

const U64_MAX_LIT = (1n << 64n) - 1n;

function requireU64(value: bigint | number, label: string): bigint {
  if (typeof value === "number") {
    if (!Number.isSafeInteger(value) || value < 0) {
      throw new RangeError(`${label} must be a safe u64 integer, got ${value}`);
    }
    return BigInt(value);
  }
  if (value < 0n || value > U64_MAX_LIT) {
    throw new RangeError(`${label} must be in [0, ${U64_MAX_LIT}], got ${value}`);
  }
  return value;
}

export const UNLIMITED_HELD_LIMITS: HeldLimits = {
  maxBondLamports: U64_MAX_LIT,
  maxTtlSlots: U64_MAX_LIT,
  maxRecoveryMode: 2,
};

export const NO_HELD_DELIVERY: HeldLimits = {
  maxBondLamports: 0n,
  maxTtlSlots: 0n,
  maxRecoveryMode: 0,
};

/** Accept a hold only while the sender itself remains the recovery authority. */
export const ORIGINATOR_RECOVERY_ONLY: HeldLimits = {
  ...UNLIMITED_HELD_LIMITS,
  maxRecoveryMode: 0,
};

/**
 * Pack TransferChecked data via the Codama encoder, after refusing inputs that would
 * silently coerce on the wire.
 */
export function encodeTransferChecked(params: {
  amount: bigint | number;
  decimals: number;
  uniqueNonce: Uint8Array;
  /**
   * Required, deliberately. Defaulting to unlimited would silently hand the destination the
   * choice of bond, lock duration and recovery authority; pass `UNLIMITED_HELD_LIMITS` to opt
   * into that explicitly.
   */
  limits: HeldLimits;
}): Uint8Array {
  if (params.uniqueNonce.length !== 32) {
    throw new Error("uniqueNonce must be 32 bytes");
  }
  const amount = requireU64(params.amount, "amount");
  const maxBondLamports = requireU64(params.limits.maxBondLamports, "maxBondLamports");
  const maxTtlSlots = requireU64(params.limits.maxTtlSlots, "maxTtlSlots");
  return new Uint8Array(
    getTransferCheckedInstructionDataEncoder().encode({
      amount,
      decimals: requireByte(params.decimals, 255, "decimals"),
      uniqueNonce: Array.from(params.uniqueNonce),
      maxBondLamports,
      maxTtlSlots,
      maxRecoveryMode: requireByte(params.limits.maxRecoveryMode, 2, "maxRecoveryMode"),
    }),
  );
}

/**
 * Pack InitializeReceivePolicy via the Codama encoder, after refusing bad modes / key lengths.
 */
export function encodeInitializeReceivePolicy(params: {
  minAmount: bigint | number;
  sourceOwnerMode: number;
  recoveryAuthorityMode: number;
  recoveryAuthority: Uint8Array;
  receiptBondLamports: bigint | number;
  receiptTtlSlots: bigint | number;
  allowlist: Uint8Array[];
}): Uint8Array {
  if (params.allowlist.length > ALLOWLIST_CAP) {
    throw new Error(`allowlist exceeds cap ${ALLOWLIST_CAP}`);
  }
  const minAmount = requireU64(params.minAmount, "minAmount");
  const receiptBondLamports = requireU64(params.receiptBondLamports, "receiptBondLamports");
  const receiptTtlSlots = requireU64(params.receiptTtlSlots, "receiptTtlSlots");
  const effectiveTtlSlots =
    receiptTtlSlots === 0n ? BigInt(DEFAULT_RECEIPT_TTL_SLOTS) : receiptTtlSlots;
  if (receiptBondLamports > BigInt(MAX_RECEIPT_BOND_LAMPORTS)) {
    throw new RangeError(
      `receiptBondLamports exceeds MAX_RECEIPT_BOND_LAMPORTS (${MAX_RECEIPT_BOND_LAMPORTS})`,
    );
  }
  if (effectiveTtlSlots > BigInt(MAX_RECEIPT_TTL_SLOTS)) {
    throw new RangeError(
      `receiptTtlSlots exceeds MAX_RECEIPT_TTL_SLOTS (${MAX_RECEIPT_TTL_SLOTS})`,
    );
  }
  return new Uint8Array(
    getInitializeReceivePolicyInstructionDataEncoder().encode({
      minAmount,
      sourceOwnerMode: requireByte(params.sourceOwnerMode, 1, "sourceOwnerMode"),
      recoveryAuthorityMode: requireByte(
        params.recoveryAuthorityMode,
        2,
        "recoveryAuthorityMode",
      ),
      recoveryAuthority: addressFromBytes(params.recoveryAuthority, "recoveryAuthority"),
      receiptBondLamports,
      receiptTtlSlots,
      allowlist: params.allowlist.map((k, i) => addressFromBytes(k, `allowlist[${i}]`)),
    }),
  );
}

/** Empty-body instructions: tag byte only (generated encoder). */
export function encodeEnsureGuard(): Uint8Array {
  return new Uint8Array(getEnsureGuardInstructionDataEncoder().encode({}));
}

export function encodeClaimReceipt(): Uint8Array {
  return new Uint8Array(getClaimReceiptInstructionDataEncoder().encode({}));
}

export function encodeCloseExpiredReceipt(): Uint8Array {
  return new Uint8Array(getCloseExpiredReceiptInstructionDataEncoder().encode({}));
}

/** Outcome reported as instruction return data. `held` still succeeds. */
export const TransferOutcome = {
  Credited: 0,
  Held: 1,
} as const;
export type TransferOutcome = (typeof TransferOutcome)[keyof typeof TransferOutcome];

/**
 * Read the outcome from a transaction's return data.
 *
 * A held transfer succeeds, so checking only that the transaction landed reads a diverted
 * payment as a delivered one. Returns `null` when no return data is present.
 *
 * Multi-ix transactions: return data is last-instruction scoped; also index the held log.
 */
export function decodeTransferOutcome(
  returnData: Uint8Array | null | undefined,
): TransferOutcome | null {
  if (!returnData || returnData.length === 0) return null;
  const byte = returnData[0];
  if (byte !== TransferOutcome.Credited && byte !== TransferOutcome.Held) {
    throw new Error(`unrecognized transfer outcome byte: ${byte}`);
  }
  return byte;
}

// —— Reading a destination's terms before paying ——

const ACCOUNT_SIZE = 165;
const ACCOUNT_TYPE_ACCOUNT = 2;
const EXTENSION_TYPE_RECEIVE_POLICY = 10_000;
const ALLOWLIST_CAP_BYTES = 8 * 32;
/** min_amount(8) modes(2) pad(6) recovery_authority(32) bond(8) ttl(8) len(1) pad(7) allowlist */
const POLICY_LEN = 8 + 1 + 1 + 6 + 32 + 8 + 8 + 1 + 7 + ALLOWLIST_CAP_BYTES;

export type ReceivePolicy = {
  minAmount: bigint;
  sourceOwnerMode: number;
  recoveryAuthorityMode: number;
  recoveryAuthority: Uint8Array;
  receiptBondLamports: bigint;
  receiptTtlSlots: bigint;
  allowlist: Uint8Array[];
};

/**
 * Decode a destination's ReceivePolicy from raw account data.
 *
 * A sender cannot choose sensible `HeldLimits` without knowing the destination's terms, and the
 * policy is write-once, so what this returns cannot change under an in-flight payment. Returns
 * `null` when the account carries no policy. Throws when the account carries a malformed one:
 * treating corruption as absence is how a policy gets bypassed.
 */
export function decodeReceivePolicy(data: Uint8Array): ReceivePolicy | null {
  if (data.length < ACCOUNT_SIZE + 1 || data[ACCOUNT_SIZE] !== ACCOUNT_TYPE_ACCOUNT) return null;
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  let cursor = ACCOUNT_SIZE + 1;
  let policy: ReceivePolicy | null = null;
  while (cursor + 4 <= data.length) {
    const type = view.getUint16(cursor, true);
    if (type === 0) return policy;
    const len = view.getUint16(cursor + 2, true);
    const start = cursor + 4;
    if (start + len > data.length) throw new Error("malformed TLV: entry overruns the account");
    if (type !== EXTENSION_TYPE_RECEIVE_POLICY) {
      throw new Error(`unsupported account extension type: ${type}`);
    }
    if (len !== POLICY_LEN) {
      throw new Error(`malformed ReceivePolicy: declared ${len} bytes, expected ${POLICY_LEN}`);
    }
    if (!policy) {
      let o = start;
      const minAmount = view.getBigUint64(o, true);
      const sourceOwnerMode = data[o + 8];
      const recoveryAuthorityMode = data[o + 9];
      o += 16;
      const recoveryAuthority = data.slice(o, o + 32);
      o += 32;
      const receiptBondLamports = view.getBigUint64(o, true);
      const receiptTtlSlots = view.getBigUint64(o + 8, true);
      const allowlistLen = data[o + 16];
      o += 24;
      const allowlist: Uint8Array[] = [];
      for (let i = 0; i < Math.min(allowlistLen, 8); i++) {
        allowlist.push(data.slice(o + i * 32, o + i * 32 + 32));
      }
      policy = {
        minAmount,
        sourceOwnerMode,
        recoveryAuthorityMode,
        recoveryAuthority,
        receiptBondLamports,
        receiptTtlSlots,
        allowlist,
      };
    }
    cursor = start + len;
  }
  return policy;
}

/** `true` only when the account genuinely carries a policy; throws on a malformed one. */
export function hasReceivePolicy(data: Uint8Array): boolean {
  return decodeReceivePolicy(data) !== null;
}

/** Byte size of a Receipt account; the bond is floored at its rent exemption on chain. */
export const RECEIPT_SIZE = 304;

export type PreviewedOutcome = "credited" | "held" | "failed";

/**
 * What will this transfer actually do?
 *
 * Lets a sender decide before paying instead of discovering it from a failed transaction or,
 * worse, a successful one that held the funds. Mirrors the on-chain order of checks, including
 * the rent floor and the zero-amount hold reject. Pass
 * `getMinimumBalanceForRentExemption(RECEIPT_SIZE)` as `rentExemptReceiptLamports`.
 */
export function previewOutcome(params: {
  policy: ReceivePolicy | null;
  amount: bigint | number;
  sourceOwner: Uint8Array;
  limits: HeldLimits;
  rentExemptReceiptLamports: bigint | number;
}): PreviewedOutcome {
  const { policy } = params;
  const amount = requireU64(params.amount, "amount");
  if (!policy) return "credited";
  if (
    (policy.sourceOwnerMode !== 0 && policy.sourceOwnerMode !== 1) ||
    !Number.isInteger(policy.recoveryAuthorityMode) ||
    policy.recoveryAuthorityMode < 0 ||
    policy.recoveryAuthorityMode > 2
  ) {
    return "failed";
  }
  const minAmount = requireU64(policy.minAmount, "policy.minAmount");
  const receiptBondLamports = requireU64(
    policy.receiptBondLamports,
    "policy.receiptBondLamports",
  );
  const receiptTtlSlots = requireU64(policy.receiptTtlSlots, "policy.receiptTtlSlots");
  const maxBondLamports = requireU64(params.limits.maxBondLamports, "maxBondLamports");
  const maxTtlSlots = requireU64(params.limits.maxTtlSlots, "maxTtlSlots");
  const maxRecoveryMode = requireByte(params.limits.maxRecoveryMode, 2, "maxRecoveryMode");

  const sameKey = (a: Uint8Array, b: Uint8Array) =>
    a.length === b.length && a.every((x, i) => x === b[i]);
  const accepts =
    amount >= minAmount &&
    (policy.sourceOwnerMode === 0 ||
      policy.allowlist.some((k) => sameKey(k, params.sourceOwner)));
  if (accepts) return "credited";

  if (amount === 0n) return "failed";

  // The held path re-checks the protocol caps at point of use, because the policy is a TLV blob
  // with no version of its own and could carry a value written before the caps existed. A
  // preview that only compares sender limits would say `held` where the chain fails.
  if (
    receiptBondLamports > BigInt(MAX_RECEIPT_BOND_LAMPORTS) ||
    receiptTtlSlots > BigInt(MAX_RECEIPT_TTL_SLOTS)
  ) {
    return "failed";
  }

  const rentFloor = requireU64(params.rentExemptReceiptLamports, "rentExemptReceiptLamports");
  const bond = receiptBondLamports > rentFloor ? receiptBondLamports : rentFloor;
  if (
    bond > maxBondLamports ||
    receiptTtlSlots > maxTtlSlots ||
    policy.recoveryAuthorityMode > maxRecoveryMode
  ) {
    return "failed";
  }
  return "held";
}

export type AccountRole = {
  address: string;
  isSigner: boolean;
  isWritable: boolean;
};

/**
 * Account metas for TransferChecked, with roles.
 *
 * Prefer `getTransferCheckedInstruction` from the generated client when you already have Kit
 * signers; this helper is for callers that only need the role list (e.g. offline assembly).
 */
export function transferCheckedAccounts(params: {
  source: string;
  mint: string;
  destination: string;
  authority: string;
  policy?: PolicyTransferAccounts;
  systemProgram?: string;
}): AccountRole[] {
  const accounts: AccountRole[] = [
    { address: params.source, isSigner: false, isWritable: true },
    { address: params.mint, isSigner: false, isWritable: false },
    { address: params.destination, isSigner: false, isWritable: true },
    { address: params.authority, isSigner: true, isWritable: false },
  ];
  if (params.policy) {
    accounts.push(
      { address: params.policy.guardToken, isSigner: false, isWritable: true },
      { address: params.policy.guardState, isSigner: false, isWritable: true },
      { address: params.policy.receipt, isSigner: false, isWritable: true },
      { address: params.policy.bondPayer, isSigner: true, isWritable: true },
      {
        address: params.systemProgram ?? "11111111111111111111111111111111",
        isSigner: false,
        isWritable: false,
      },
    );
  }
  return accounts;
}

/** Generated PDA finder with the nonce width check the generated bytes encoder omits. */
export function findReceiptPdaChecked(
  seeds: ReceiptSeeds,
  config: { programAddress?: Address | undefined } = {},
): Promise<ProgramDerivedAddress> {
  if (seeds.uniqueNonce.length !== 32) {
    throw new Error(`uniqueNonce must be 32 bytes, got ${seeds.uniqueNonce.length}`);
  }
  return findReceiptPdaGenerated(seeds, config);
}

/** Addresses only, in instruction order. See `transferCheckedAccounts` for roles. */
export function transferCheckedAccountKeys(params: {
  source: string;
  mint: string;
  destination: string;
  authority: string;
  policy?: PolicyTransferAccounts;
  systemProgram?: string;
}): string[] {
  return transferCheckedAccounts(params).map((a) => a.address);
}
