import {
  guardStateSeeds,
  guardTokenSeeds,
  receiptSeeds,
  PROGRAM_ID,
} from "./constants.js";

export type AddressApi = {
  getProgramDerivedAddress: (input: {
    programAddress: string;
    seeds: Uint8Array[];
  }) => Promise<readonly [string, number]>;
};

export async function deriveGuardTokenAddress(
  api: AddressApi,
  receiver: Uint8Array,
  mint: Uint8Array,
  programId: string = PROGRAM_ID,
): Promise<[string, number]> {
  const [address, bump] = await api.getProgramDerivedAddress({
    programAddress: programId,
    seeds: guardTokenSeeds(receiver, mint),
  });
  return [address, bump];
}

export async function deriveGuardStateAddress(
  api: AddressApi,
  receiver: Uint8Array,
  mint: Uint8Array,
  programId: string = PROGRAM_ID,
): Promise<[string, number]> {
  const [address, bump] = await api.getProgramDerivedAddress({
    programAddress: programId,
    seeds: guardStateSeeds(receiver, mint),
  });
  return [address, bump];
}

export async function deriveReceiptAddress(
  api: AddressApi,
  receiver: Uint8Array,
  mint: Uint8Array,
  sourceOwner: Uint8Array,
  uniqueNonce: Uint8Array,
  programId: string = PROGRAM_ID,
): Promise<[string, number]> {
  const [address, bump] = await api.getProgramDerivedAddress({
    programAddress: programId,
    seeds: receiptSeeds(receiver, mint, sourceOwner, uniqueNonce),
  });
  return [address, bump];
}
