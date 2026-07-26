/**
 * Minimal Kit-oriented helpers for the token-2022-receive reference program.
 * Not a full SDK — enough to derive PDAs and encode TransferChecked for local exercise.
 */

export {
  PROGRAM_ID,
  MAX_OPEN_RECEIPTS,
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

export function encodeTransferChecked(params: {
  amount: bigint | number;
  decimals: number;
  uniqueNonce: Uint8Array;
}): Uint8Array {
  if (params.uniqueNonce.length !== 32) {
    throw new Error("uniqueNonce must be 32 bytes");
  }
  const out = new Uint8Array(1 + 8 + 1 + 32);
  out[0] = Ix.TransferChecked;
  out.set(encodeU64LE(params.amount), 1);
  out[9] = params.decimals;
  out.set(params.uniqueNonce, 10);
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
  if (params.sourceOwnerMode > 1) {
    throw new RangeError(`sourceOwnerMode must be 0 or 1, got ${params.sourceOwnerMode}`);
  }
  if (params.recoveryAuthorityMode > 2) {
    throw new RangeError(
      `recoveryAuthorityMode must be 0, 1 or 2, got ${params.recoveryAuthorityMode}`,
    );
  }
  const parts: Uint8Array[] = [
    Uint8Array.of(Ix.InitializeReceivePolicy),
    encodeU64LE(params.minAmount),
    Uint8Array.of(params.sourceOwnerMode, params.recoveryAuthorityMode),
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
