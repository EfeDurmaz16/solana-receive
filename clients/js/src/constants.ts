export const PROGRAM_ID =
  "GyrTVV4hbcuzJuSz86FNq7K2UVAoSJQtcgHTVTz1hPPq" as const;

export const DEFAULT_RECEIPT_TTL_SLOTS = 1_512_000;
/** Protocol ceilings, enforced on-chain at InitializeReceivePolicy. */
export const MAX_RECEIPT_TTL_SLOTS = 6_480_000;
export const MAX_RECEIPT_BOND_LAMPORTS = 1_000_000_000;
export const ALLOWLIST_CAP = 8;

export const SEEDS = {
  guard: "guard",
  guardState: "guard-state",
  receipt: "receipt",
} as const;

const encoder = new TextEncoder();

/** A wrong-length seed silently derives a different address, so check every one. */
function seed(bytes: Uint8Array, label: string): Uint8Array {
  if (bytes.length !== 32) {
    throw new Error(`${label} must be 32 bytes, got ${bytes.length}`);
  }
  return bytes;
}

export function guardTokenSeeds(receiver: Uint8Array, mint: Uint8Array): Uint8Array[] {
  return [encoder.encode(SEEDS.guard), seed(receiver, "receiver"), seed(mint, "mint")];
}

export function guardStateSeeds(receiver: Uint8Array, mint: Uint8Array): Uint8Array[] {
  return [encoder.encode(SEEDS.guardState), seed(receiver, "receiver"), seed(mint, "mint")];
}

export function receiptSeeds(
  receiver: Uint8Array,
  mint: Uint8Array,
  sourceOwner: Uint8Array,
  uniqueNonce: Uint8Array,
): Uint8Array[] {
  if (uniqueNonce.length !== 32) throw new Error("uniqueNonce must be 32 bytes");
  return [
    encoder.encode(SEEDS.receipt),
    seed(receiver, "receiver"),
    seed(mint, "mint"),
    seed(sourceOwner, "sourceOwner"),
    uniqueNonce,
  ];
}
