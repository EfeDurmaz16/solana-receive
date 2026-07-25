/**
 * Minimal Kit-oriented helpers for the token-2022-receive reference program.
 * Not a full SDK — enough to derive PDAs and encode TransferChecked for local exercise.
 */

export {
  PROGRAM_ID,
  MAX_OPEN_RECEIPTS,
  DEFAULT_RECEIPT_TTL_SLOTS,
  ALLOWLIST_CAP,
  SEEDS,
  guardTokenSeeds,
  guardStateSeeds,
  receiptSeeds,
} from "./constants.js";

import { ALLOWLIST_CAP } from "./constants.js";

export {
  deriveGuardTokenAddress,
  deriveGuardStateAddress,
  deriveReceiptAddress,
} from "./pda.js";
export type { AddressApi } from "./pda.js";

/** Instruction tags — keep in sync with Rust `ReceiveTokenInstruction`. */
export enum Ix {
  InitializeMint2 = 0,
  InitializeAccount3 = 1,
  InitializeReceivePolicy = 2,
  EnsureGuard = 3,
  TransferChecked = 4,
  ClaimReceipt = 5,
  CloseExpiredReceipt = 6,
  MintTo = 7,
}

export type PolicyTransferAccounts = {
  guardToken: string;
  guardState: string;
  receipt: string;
  bondPayer: string;
};

export function encodeU64LE(n: bigint | number): Uint8Array {
  const v = BigInt(n);
  const out = new Uint8Array(8);
  const view = new DataView(out.buffer);
  view.setBigUint64(0, v, true);
  return out;
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
  const parts: Uint8Array[] = [
    Uint8Array.of(Ix.InitializeReceivePolicy),
    encodeU64LE(params.minAmount),
    Uint8Array.of(params.sourceOwnerMode, params.recoveryAuthorityMode),
    params.recoveryAuthority,
    encodeU64LE(params.receiptBondLamports),
    encodeU64LE(params.receiptTtlSlots),
    Uint8Array.of(params.allowlist.length),
    ...params.allowlist,
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

/**
 * Account metas for TransferChecked.
 * Pass `policy` only when the destination has ReceivePolicy enabled.
 */
export function transferCheckedAccountKeys(params: {
  source: string;
  mint: string;
  destination: string;
  authority: string;
  policy?: PolicyTransferAccounts;
  systemProgram?: string;
}): string[] {
  const keys = [
    params.source,
    params.mint,
    params.destination,
    params.authority,
  ];
  if (params.policy) {
    keys.push(
      params.policy.guardToken,
      params.policy.guardState,
      params.policy.receipt,
      params.policy.bondPayer,
      params.systemProgram ?? "11111111111111111111111111111111",
    );
  }
  return keys;
}
