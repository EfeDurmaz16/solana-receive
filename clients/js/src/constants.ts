export const PROGRAM_ID =
  "GyrTVV4hbcuzJuSz86FNq7K2UVAoSJQtcgHTVTz1hPPq" as const;

export const MAX_OPEN_RECEIPTS = 64;
export const DEFAULT_RECEIPT_TTL_SLOTS = 1_512_000;
export const ALLOWLIST_CAP = 8;

export const SEEDS = {
  guard: "guard",
  guardState: "guard-state",
  receipt: "receipt",
} as const;

const encoder = new TextEncoder();

export function guardTokenSeeds(receiver: Uint8Array, mint: Uint8Array): Uint8Array[] {
  return [encoder.encode(SEEDS.guard), receiver, mint];
}

export function guardStateSeeds(receiver: Uint8Array, mint: Uint8Array): Uint8Array[] {
  return [encoder.encode(SEEDS.guardState), receiver, mint];
}

export function receiptSeeds(
  receiver: Uint8Array,
  mint: Uint8Array,
  sourceOwner: Uint8Array,
  uniqueNonce: Uint8Array,
): Uint8Array[] {
  if (uniqueNonce.length !== 32) throw new Error("uniqueNonce must be 32 bytes");
  return [encoder.encode(SEEDS.receipt), receiver, mint, sourceOwner, uniqueNonce];
}
