/**
 * Minimal Kit-oriented helpers for the token-2022-receive reference program.
 * Not a full SDK — enough to derive PDAs and encode TransferChecked for local exercise.
 */

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

import { ALLOWLIST_CAP } from "./constants.ts";

export {
  deriveGuardTokenAddress,
  deriveGuardStateAddress,
  deriveReceiptAddress,
} from "./pda.ts";
export type { AddressApi } from "./pda.ts";

/**
 * Instruction tags - keep in sync with Rust `ReceiveTokenInstruction`.
 *
 * A const object rather than a TS `enum`: enums emit runtime code, so they are not erasable
 * and cannot be run by type-stripping runtimes (node --experimental-strip-types, bun, tsx).
 */
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

const U64_MAX = (1n << 64n) - 1n;

export function encodeU64LE(n: bigint | number): Uint8Array {
  const v = BigInt(n);
  if (v < 0n || v > U64_MAX) {
    throw new RangeError(`value out of u64 range: ${v}`);
  }
  const out = new Uint8Array(8);
  const view = new DataView(out.buffer);
  view.setBigUint64(0, v, true);
  return out;
}

/** Pubkeys are fixed-width on the wire; a short one shifts every field after it. */
function requirePubkey(bytes: Uint8Array, label: string): Uint8Array {
  if (bytes.length !== 32) {
    throw new Error(`${label} must be 32 bytes, got ${bytes.length}`);
  }
  return bytes;
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
  const out = new Uint8Array(1 + 8 + 1 + 32 + 8 + 8 + 1);
  out[0] = Ix.TransferChecked;
  out.set(encodeU64LE(params.amount), 1);
  out[9] = requireByte(params.decimals, 255, "decimals");
  out.set(params.uniqueNonce, 10);
  out.set(encodeU64LE(params.limits.maxBondLamports), 42);
  out.set(encodeU64LE(params.limits.maxTtlSlots), 50);
  out[58] = requireByte(params.limits.maxRecoveryMode, 2, "maxRecoveryMode");
  return out;
}

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
  const parts: Uint8Array[] = [
    Uint8Array.of(Ix.InitializeReceivePolicy),
    encodeU64LE(params.minAmount),
    Uint8Array.of(
      requireByte(params.sourceOwnerMode, 1, "sourceOwnerMode"),
      requireByte(params.recoveryAuthorityMode, 2, "recoveryAuthorityMode"),
    ),
    requirePubkey(params.recoveryAuthority, "recoveryAuthority"),
    encodeU64LE(params.receiptBondLamports),
    encodeU64LE(params.receiptTtlSlots),
    Uint8Array.of(params.allowlist.length),
    ...params.allowlist.map((k, i) => requirePubkey(k, `allowlist[${i}]`)),
  ];
  const len = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(len);
  let o = 0;
  for (const p of parts) {
    out.set(p, o);
    o += p.length;
  }
  return out;
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
  while (cursor + 4 <= data.length) {
    const type = view.getUint16(cursor, true);
    const len = view.getUint16(cursor + 2, true);
    const start = cursor + 4;
    if (start + len > data.length) throw new Error("malformed TLV: entry overruns the account");
    if (type === EXTENSION_TYPE_RECEIVE_POLICY) {
      if (len !== POLICY_LEN) {
        throw new Error(`malformed ReceivePolicy: declared ${len} bytes, expected ${POLICY_LEN}`);
      }
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
      return {
        minAmount,
        sourceOwnerMode,
        recoveryAuthorityMode,
        recoveryAuthority,
        receiptBondLamports,
        receiptTtlSlots,
        allowlist,
      };
    }
    if (type === 0) return null;
    cursor = start + len;
  }
  return null;
}

/** `true` only when the account genuinely carries a policy; throws on a malformed one. */
export function hasReceivePolicy(data: Uint8Array): boolean {
  return decodeReceivePolicy(data) !== null;
}

/**
 * Would this policy accept `amount` from `sourceOwner`, and would a hold meet `limits`?
 *
 * Lets a sender decide before paying instead of discovering it from a failed transaction or,
 * worse, a successful one that held the funds.
 */
export function previewOutcome(params: {
  policy: ReceivePolicy | null;
  amount: bigint | number;
  sourceOwner: Uint8Array;
  limits: HeldLimits;
}): "credited" | "held" | "rejected-by-sender-limits" {
  const { policy } = params;
  if (!policy) return "credited";
  const amount = BigInt(params.amount);
  const sameKey = (a: Uint8Array, b: Uint8Array) =>
    a.length === b.length && a.every((x, i) => x === b[i]);
  const accepts =
    amount >= policy.minAmount &&
    (policy.sourceOwnerMode === 0 ||
      policy.allowlist.some((k) => sameKey(k, params.sourceOwner)));
  if (accepts) return "credited";
  if (
    policy.receiptBondLamports > BigInt(params.limits.maxBondLamports) ||
    policy.receiptTtlSlots > BigInt(params.limits.maxTtlSlots) ||
    policy.recoveryAuthorityMode > params.limits.maxRecoveryMode
  ) {
    return "rejected-by-sender-limits";
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
 * Roles are not cosmetic: `bond_payer` must be a writable signer and five other accounts must
 * be writable, and a caller that guessed wrong would get an opaque runtime failure.
 * Pass `policy` only when the destination has ReceivePolicy enabled.
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
