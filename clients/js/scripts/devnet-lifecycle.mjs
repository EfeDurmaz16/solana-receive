#!/usr/bin/env node
/**
 * Devnet lifecycle: ordinary SPL (before) + receive-policy credited/held/claim/expiry.
 *
 * Uses the funded Solana CLI keypair (no faucet airdrop). Waits on real slots for
 * expiry (no Surfpool time-travel). Writes demos/receive/last-run.json for the UI.
 *
 * Env:
 *   RECEIVE_PROGRAM_ID   deployed program pubkey (required if not declared ID)
 *   RECEIVE_RPC_URL      default https://api.devnet.solana.com
 *   RECEIVE_WS_URL       default wss://api.devnet.solana.com
 *   RECEIVE_KEYPAIR      path to payer keypair JSON (default ~/.config/solana/id.json)
 */
import { readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  AccountRole,
  address,
  appendTransactionMessageInstructions,
  createKeyPairSignerFromBytes,
  createSolanaRpc,
  createSolanaRpcSubscriptions,
  createTransactionMessage,
  generateKeyPairSigner,
  getAddressEncoder,
  getBase64EncodedWireTransaction,
  getSignatureFromTransaction,
  none,
  pipe,
  sendAndConfirmTransactionFactory,
  setTransactionMessageFeePayerSigner,
  setTransactionMessageLifetimeUsingBlockhash,
  signTransactionMessageWithSigners,
} from "@solana/kit";

const __dirname = dirname(fileURLToPath(import.meta.url));
const clientRoot = resolve(__dirname, "../src/index.ts");
const {
  PROGRAM_ID: DECLARED_PROGRAM_ID,
  RECEIPT_SIZE,
  UNLIMITED_HELD_LIMITS,
  decodeReceivePolicy,
  decodeTransferOutcome,
  findGuardStatePda,
  findGuardTokenPda,
  findReceiptPda,
  getClaimReceiptInstruction,
  getCloseExpiredReceiptInstruction,
  getEnsureGuardInstruction,
  getInitializeAccount3Instruction,
  getInitializeMint2Instruction,
  getInitializeReceivePolicyInstruction,
  getMintToInstruction,
  getTransferCheckedInstruction,
  previewOutcome,
  TransferOutcome,
} = await import(pathToFileURL(clientRoot).href);

const RPC_URL = process.env.RECEIVE_RPC_URL ?? "https://api.devnet.solana.com";
const WS_URL = process.env.RECEIVE_WS_URL ?? "wss://api.devnet.solana.com";
const PROGRAM_ID = address(process.env.RECEIVE_PROGRAM_ID ?? DECLARED_PROGRAM_ID);
const KEYPAIR_PATH =
  process.env.RECEIVE_KEYPAIR ?? resolve(homedir(), ".config/solana/id.json");
const SYSTEM_PROGRAM = address("11111111111111111111111111111111");
const TOKEN_PROGRAM = address("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const DECIMALS = 6;
const MIN_AMOUNT = 100n;
/** Short TTL so expiry can be demonstrated by waiting on Devnet slots. */
const DEMO_TTL_SLOTS = 32n;
const BOND_LAMPORTS = 1_000_000n;
const MINT_SIZE = 82;
const ACCOUNT_SIZE = 165;
const ACCOUNT_WITH_POLICY_SIZE = 498;
const ARTIFACT_PATH = resolve(__dirname, "../../../demos/receive/last-run.json");

const rpc = createSolanaRpc(RPC_URL);
const rpcSubscriptions = createSolanaRpcSubscriptions(WS_URL);
const sendAndConfirm = sendAndConfirmTransactionFactory({ rpc, rpcSubscriptions });
const programConfig = { programAddress: PROGRAM_ID };

function clearEvidenceArtifact() {
  try {
    unlinkSync(ARTIFACT_PATH);
  } catch (e) {
    if (e && e.code !== "ENOENT") throw e;
  }
}

function log(step, detail) {
  console.log(`\n== ${step} ==`);
  if (detail) console.log(detail);
}

function explorerTx(sig) {
  return `https://explorer.solana.com/tx/${sig}?cluster=devnet`;
}

async function loadPayer() {
  const raw = JSON.parse(readFileSync(KEYPAIR_PATH, "utf8"));
  if (!Array.isArray(raw) || raw.length < 64) {
    throw new Error(`expected secret key byte array in ${KEYPAIR_PATH}`);
  }
  return createKeyPairSignerFromBytes(Uint8Array.from(raw));
}

function getCreateAccountInstruction({ payer, newAccount, lamports, space, owner }) {
  const data = new Uint8Array(4 + 8 + 8 + 32);
  const view = new DataView(data.buffer);
  view.setUint32(0, 0, true);
  view.setBigUint64(4, BigInt(lamports), true);
  view.setBigUint64(12, BigInt(space), true);
  data.set(getAddressEncoder().encode(owner), 20);
  return {
    programAddress: SYSTEM_PROGRAM,
    accounts: [
      { address: payer.address, role: AccountRole.WRITABLE_SIGNER, signer: payer },
      {
        address: newAccount.address,
        role: AccountRole.WRITABLE_SIGNER,
        signer: newAccount,
      },
    ],
    data,
  };
}

/** Tokenkeg InitializeMint2 (ix 20): decimals u8 + mint_authority pubkey + freeze COption=None */
function getTokenkegInitializeMint2({ mint, decimals, mintAuthority }) {
  const data = new Uint8Array(1 + 1 + 32 + 1);
  data[0] = 20;
  data[1] = decimals;
  data.set(getAddressEncoder().encode(mintAuthority), 2);
  data[34] = 0; // COption::None
  return {
    programAddress: TOKEN_PROGRAM,
    accounts: [{ address: mint, role: AccountRole.WRITABLE }],
    data,
  };
}

/** Tokenkeg InitializeAccount3 (ix 18): owner pubkey */
function getTokenkegInitializeAccount3({ account, mint, owner }) {
  const data = new Uint8Array(1 + 32);
  data[0] = 18;
  data.set(getAddressEncoder().encode(owner), 1);
  return {
    programAddress: TOKEN_PROGRAM,
    accounts: [
      { address: account, role: AccountRole.WRITABLE },
      { address: mint, role: AccountRole.READONLY },
    ],
    data,
  };
}

/** Tokenkeg MintTo (ix 7): amount u64 */
function getTokenkegMintTo({ mint, account, mintAuthority, amount }) {
  const data = new Uint8Array(1 + 8);
  data[0] = 7;
  new DataView(data.buffer).setBigUint64(1, amount, true);
  return {
    programAddress: TOKEN_PROGRAM,
    accounts: [
      { address: mint, role: AccountRole.WRITABLE },
      { address: account, role: AccountRole.WRITABLE },
      {
        address: mintAuthority.address,
        role: AccountRole.READONLY_SIGNER,
        signer: mintAuthority,
      },
    ],
    data,
  };
}

/** Tokenkeg TransferChecked (ix 12): amount u64 + decimals u8 */
function getTokenkegTransferChecked({
  source,
  mint,
  destination,
  authority,
  amount,
  decimals,
}) {
  const data = new Uint8Array(1 + 8 + 1);
  data[0] = 12;
  new DataView(data.buffer).setBigUint64(1, amount, true);
  data[9] = decimals;
  return {
    programAddress: TOKEN_PROGRAM,
    accounts: [
      { address: source, role: AccountRole.WRITABLE },
      { address: mint, role: AccountRole.READONLY },
      { address: destination, role: AccountRole.WRITABLE },
      {
        address: authority.address,
        role: AccountRole.READONLY_SIGNER,
        signer: authority,
      },
    ],
    data,
  };
}

async function sendIx(label, feePayer, instructions) {
  const { value: latest } = await rpc.getLatestBlockhash().send();
  const message = pipe(
    createTransactionMessage({ version: 0 }),
    (tx) => setTransactionMessageFeePayerSigner(feePayer, tx),
    (tx) => setTransactionMessageLifetimeUsingBlockhash(latest, tx),
    (tx) => appendTransactionMessageInstructions(instructions, tx),
  );
  const signed = await signTransactionMessageWithSigners(message);
  try {
    await sendAndConfirm(signed, { commitment: "confirmed" });
  } catch (e) {
    const wire = getBase64EncodedWireTransaction(signed);
    console.error(`failed at ${label}; wire(b64) len=${wire.length}`);
    throw e;
  }
  return getSignatureFromTransaction(signed);
}

async function tokenAmount(account) {
  const info = await rpc.getAccountInfo(account, { encoding: "base64" }).send();
  if (!info.value) return 0n;
  const raw = Buffer.from(info.value.data[0], "base64");
  return new DataView(raw.buffer, raw.byteOffset, raw.byteLength).getBigUint64(64, true);
}

async function waitUntilSlot(minSlot) {
  for (;;) {
    const slot = BigInt(await rpc.getSlot().send());
    if (slot >= minSlot) return slot;
    await new Promise((r) => setTimeout(r, 400));
  }
}

async function main() {
  clearEvidenceArtifact();
  const payer = await loadPayer();
  log(
    "devnet lifecycle",
    `rpc=${RPC_URL}\nprogram=${PROGRAM_ID}\npayer=${payer.address}`,
  );

  await rpc.getVersion().send();

  const mintAuthority = await generateKeyPairSigner();
  const sourceOwner = await generateKeyPairSigner();
  const destOwner = await generateKeyPairSigner();
  const mint = await generateKeyPairSigner();
  const source = await generateKeyPairSigner();
  const destination = await generateKeyPairSigner();
  const claimDest = await generateKeyPairSigner();

  // --- BEFORE: ordinary SPL Tokenkeg always-credits ---
  const splMint = await generateKeyPairSigner();
  const splSource = await generateKeyPairSigner();
  const splDest = await generateKeyPairSigner();
  const splOwner = await generateKeyPairSigner();

  const mintRent = await rpc.getMinimumBalanceForRentExemption(BigInt(MINT_SIZE)).send();
  const accountRent = await rpc.getMinimumBalanceForRentExemption(BigInt(ACCOUNT_SIZE)).send();
  const policyRent = await rpc
    .getMinimumBalanceForRentExemption(BigInt(ACCOUNT_WITH_POLICY_SIZE))
    .send();
  const receiptRent = await rpc.getMinimumBalanceForRentExemption(BigInt(RECEIPT_SIZE)).send();

  await sendIx("SPL create+init mint", payer, [
    getCreateAccountInstruction({
      payer,
      newAccount: splMint,
      lamports: mintRent,
      space: MINT_SIZE,
      owner: TOKEN_PROGRAM,
    }),
    getTokenkegInitializeMint2({
      mint: splMint.address,
      decimals: DECIMALS,
      mintAuthority: payer.address,
    }),
  ]);
  await sendIx("SPL create+init source/dest", payer, [
    getCreateAccountInstruction({
      payer,
      newAccount: splSource,
      lamports: accountRent,
      space: ACCOUNT_SIZE,
      owner: TOKEN_PROGRAM,
    }),
    getTokenkegInitializeAccount3({
      account: splSource.address,
      mint: splMint.address,
      owner: payer.address,
    }),
    getCreateAccountInstruction({
      payer,
      newAccount: splDest,
      lamports: accountRent,
      space: ACCOUNT_SIZE,
      owner: TOKEN_PROGRAM,
    }),
    getTokenkegInitializeAccount3({
      account: splDest.address,
      mint: splMint.address,
      owner: splOwner.address,
    }),
  ]);
  await sendIx("SPL mint_to source", payer, [
    getTokenkegMintTo({
      mint: splMint.address,
      account: splSource.address,
      mintAuthority: payer,
      amount: 1_000n,
    }),
  ]);
  const beforeSig = await sendIx("SPL TransferChecked (before / always credits)", payer, [
    getTokenkegTransferChecked({
      source: splSource.address,
      mint: splMint.address,
      destination: splDest.address,
      authority: payer,
      amount: 50n,
      decimals: DECIMALS,
    }),
  ]);
  const splDestBal = await tokenAmount(splDest.address);
  if (splDestBal !== 50n) {
    throw new Error(`SPL before: dest want 50 got ${splDestBal}`);
  }
  log("before (ordinary SPL)", `sig=${beforeSig}\ndest=${splDestBal}\n${explorerTx(beforeSig)}`);

  // --- AFTER: receive-policy program ---
  await sendIx("create+init mint", payer, [
    getCreateAccountInstruction({
      payer,
      newAccount: mint,
      lamports: mintRent,
      space: MINT_SIZE,
      owner: PROGRAM_ID,
    }),
    getInitializeMint2Instruction(
      {
        mint: mint.address,
        decimals: DECIMALS,
        mintAuthority: mintAuthority.address,
        freezeAuthority: none(),
      },
      programConfig,
    ),
  ]);

  await sendIx("create+init source", payer, [
    getCreateAccountInstruction({
      payer,
      newAccount: source,
      lamports: accountRent,
      space: ACCOUNT_SIZE,
      owner: PROGRAM_ID,
    }),
    getInitializeAccount3Instruction(
      {
        account: source.address,
        mint: mint.address,
        owner: sourceOwner.address,
      },
      programConfig,
    ),
  ]);

  await sendIx("mint_to source", payer, [
    getMintToInstruction(
      {
        mint: mint.address,
        account: source.address,
        mintAuthority,
        amount: 1_000_000n,
      },
      programConfig,
    ),
  ]);
  log("source funded", `amount=${await tokenAmount(source.address)}`);

  await sendIx("create+init destination", payer, [
    getCreateAccountInstruction({
      payer,
      newAccount: destination,
      lamports: policyRent,
      space: ACCOUNT_WITH_POLICY_SIZE,
      owner: PROGRAM_ID,
    }),
    getInitializeAccount3Instruction(
      {
        account: destination.address,
        mint: mint.address,
        owner: destOwner.address,
      },
      programConfig,
    ),
  ]);

  await sendIx("InitializeReceivePolicy", payer, [
    getInitializeReceivePolicyInstruction(
      {
        tokenAccount: destination.address,
        owner: destOwner,
        minAmount: MIN_AMOUNT,
        sourceOwnerMode: 0,
        recoveryAuthorityMode: 0,
        recoveryAuthority: sourceOwner.address,
        receiptBondLamports: BOND_LAMPORTS,
        receiptTtlSlots: DEMO_TTL_SLOTS,
        allowlist: [],
      },
      programConfig,
    ),
  ]);

  const destInfo = await rpc.getAccountInfo(destination.address, { encoding: "base64" }).send();
  const destBytes = Buffer.from(destInfo.value.data[0], "base64");
  const policy = decodeReceivePolicy(destBytes);
  if (!policy) throw new Error("destination missing ReceivePolicy after init");

  const [guardToken] = await findGuardTokenPda(
    { receiver: destOwner.address, mint: mint.address },
    programConfig,
  );
  const [guardState] = await findGuardStatePda(
    { receiver: destOwner.address, mint: mint.address },
    programConfig,
  );

  await sendIx("EnsureGuard", payer, [
    getEnsureGuardInstruction(
      {
        payer,
        receiver: destOwner.address,
        mint: mint.address,
        guardToken,
        guardState,
        systemProgram: SYSTEM_PROGRAM,
      },
      programConfig,
    ),
  ]);
  log("policy + guard ready", `guardToken=${guardToken}\nguardState=${guardState}`);

  const creditedPreview = previewOutcome({
    policy,
    amount: 150n,
    sourceOwner: getAddressEncoder().encode(sourceOwner.address),
    limits: UNLIMITED_HELD_LIMITS,
    rentExemptReceiptLamports: receiptRent,
  });
  if (creditedPreview !== "credited") {
    throw new Error(`preview expected credited, got ${creditedPreview}`);
  }

  const creditedNonce = crypto.getRandomValues(new Uint8Array(32));
  const [creditedReceipt] = await findReceiptPda(
    {
      receiver: destOwner.address,
      mint: mint.address,
      sourceOwner: sourceOwner.address,
      uniqueNonce: creditedNonce,
    },
    programConfig,
  );

  const creditedSig = await sendIx("TransferChecked credited", payer, [
    getTransferCheckedInstruction(
      {
        source: source.address,
        mint: mint.address,
        destination: destination.address,
        authority: sourceOwner,
        guardToken,
        guardState,
        receipt: creditedReceipt,
        bondPayer: payer,
        systemProgram: SYSTEM_PROGRAM,
        amount: 150n,
        decimals: DECIMALS,
        uniqueNonce: Array.from(creditedNonce),
        maxBondLamports: UNLIMITED_HELD_LIMITS.maxBondLamports,
        maxTtlSlots: UNLIMITED_HELD_LIMITS.maxTtlSlots,
        maxRecoveryMode: UNLIMITED_HELD_LIMITS.maxRecoveryMode,
      },
      programConfig,
    ),
  ]);
  const destAfterCredit = await tokenAmount(destination.address);
  if (destAfterCredit !== 150n) {
    throw new Error(`credited path: dest want 150 got ${destAfterCredit}`);
  }
  log("credited", `sig=${creditedSig}\ndest=${destAfterCredit}\n${explorerTx(creditedSig)}`);

  const heldPreview = previewOutcome({
    policy,
    amount: 1n,
    sourceOwner: getAddressEncoder().encode(sourceOwner.address),
    limits: UNLIMITED_HELD_LIMITS,
    rentExemptReceiptLamports: receiptRent,
  });
  if (heldPreview !== "held") {
    throw new Error(`preview expected held, got ${heldPreview}`);
  }

  const heldNonce = crypto.getRandomValues(new Uint8Array(32));
  const [heldReceipt] = await findReceiptPda(
    {
      receiver: destOwner.address,
      mint: mint.address,
      sourceOwner: sourceOwner.address,
      uniqueNonce: heldNonce,
    },
    programConfig,
  );

  const heldSig = await sendIx("TransferChecked held", payer, [
    getTransferCheckedInstruction(
      {
        source: source.address,
        mint: mint.address,
        destination: destination.address,
        authority: sourceOwner,
        guardToken,
        guardState,
        receipt: heldReceipt,
        bondPayer: payer,
        systemProgram: SYSTEM_PROGRAM,
        amount: 1n,
        decimals: DECIMALS,
        uniqueNonce: Array.from(heldNonce),
        maxBondLamports: UNLIMITED_HELD_LIMITS.maxBondLamports,
        maxTtlSlots: UNLIMITED_HELD_LIMITS.maxTtlSlots,
        maxRecoveryMode: UNLIMITED_HELD_LIMITS.maxRecoveryMode,
      },
      programConfig,
    ),
  ]);

  const guardAfterHold = await tokenAmount(guardToken);
  if (guardAfterHold !== 1n) {
    throw new Error(`held path: guard want 1 got ${guardAfterHold}`);
  }
  log("held", `sig=${heldSig}\nguard=${guardAfterHold}\nreceipt=${heldReceipt}\n${explorerTx(heldSig)}`);

  let heldReturnData = null;
  try {
    const tx = await rpc
      .getTransaction(heldSig, { encoding: "json", maxSupportedTransactionVersion: 0 })
      .send();
    const rd = tx?.meta?.returnData?.data?.[0];
    if (rd) {
      const bytes = Buffer.from(rd, "base64");
      const outcome = decodeTransferOutcome(bytes);
      if (outcome !== TransferOutcome.Held) {
        throw new Error(`return data want Held got ${outcome}`);
      }
      heldReturnData = outcome;
      log("held return data", `byte=${outcome}`);
    }
  } catch (e) {
    const msg = String(e?.message ?? e);
    if (msg.startsWith("return data want") || msg.startsWith("unrecognized transfer outcome")) {
      throw e;
    }
    log("held return data", `skip: ${msg}`);
  }

  await sendIx("create claim dest", payer, [
    getCreateAccountInstruction({
      payer,
      newAccount: claimDest,
      lamports: accountRent,
      space: ACCOUNT_SIZE,
      owner: PROGRAM_ID,
    }),
    getInitializeAccount3Instruction(
      {
        account: claimDest.address,
        mint: mint.address,
        owner: sourceOwner.address,
      },
      programConfig,
    ),
  ]);

  const claimSig = await sendIx("ClaimReceipt", payer, [
    getClaimReceiptInstruction(
      {
        receipt: heldReceipt,
        guardToken,
        guardState,
        claimDestination: claimDest.address,
        mint: mint.address,
        claimAuthority: sourceOwner,
        bondDest: payer.address,
      },
      programConfig,
    ),
  ]);
  const claimBal = await tokenAmount(claimDest.address);
  if (claimBal !== 1n) throw new Error(`claim want 1 got ${claimBal}`);
  log("claimed", `sig=${claimSig}\nclaimDest=${claimBal}\n${explorerTx(claimSig)}`);

  const expiryNonce = crypto.getRandomValues(new Uint8Array(32));
  const [expiryReceipt] = await findReceiptPda(
    {
      receiver: destOwner.address,
      mint: mint.address,
      sourceOwner: sourceOwner.address,
      uniqueNonce: expiryNonce,
    },
    programConfig,
  );

  const sourceBeforeExpiryHold = await tokenAmount(source.address);
  const guardBeforeExpiryHold = await tokenAmount(guardToken);

  const expiryHoldSig = await sendIx("TransferChecked held (expiry case)", payer, [
    getTransferCheckedInstruction(
      {
        source: source.address,
        mint: mint.address,
        destination: destination.address,
        authority: sourceOwner,
        guardToken,
        guardState,
        receipt: expiryReceipt,
        bondPayer: payer,
        systemProgram: SYSTEM_PROGRAM,
        amount: 1n,
        decimals: DECIMALS,
        uniqueNonce: Array.from(expiryNonce),
        maxBondLamports: UNLIMITED_HELD_LIMITS.maxBondLamports,
        maxTtlSlots: UNLIMITED_HELD_LIMITS.maxTtlSlots,
        maxRecoveryMode: UNLIMITED_HELD_LIMITS.maxRecoveryMode,
      },
      programConfig,
    ),
  ]);

  const guardAfterExpiryHold = await tokenAmount(guardToken);
  if (guardAfterExpiryHold !== guardBeforeExpiryHold + 1n) {
    throw new Error(
      `expiry hold: guard want ${guardBeforeExpiryHold + 1n} got ${guardAfterExpiryHold}`,
    );
  }

  const slotBefore = BigInt(await rpc.getSlot().send());
  const minExpiredSlot = slotBefore + DEMO_TTL_SLOTS + 1n;
  log("waiting for TTL expiry on Devnet", `from=${slotBefore} need>=${minExpiredSlot}`);
  const slotAfter = await waitUntilSlot(minExpiredSlot);
  log("TTL elapsed", `slot=${slotAfter}`);

  const expiryCloseSig = await sendIx("CloseExpiredReceipt", payer, [
    getCloseExpiredReceiptInstruction(
      {
        receipt: expiryReceipt,
        guardToken,
        guardState,
        sourceOwnerToken: source.address,
        mint: mint.address,
        bondDest: payer.address,
      },
      programConfig,
    ),
  ]);

  const sourceAfterExpiry = await tokenAmount(source.address);
  const guardAfterExpiry = await tokenAmount(guardToken);
  if (sourceAfterExpiry !== sourceBeforeExpiryHold) {
    throw new Error(
      `expiry close: source want ${sourceBeforeExpiryHold} got ${sourceAfterExpiry}`,
    );
  }
  if (guardAfterExpiry !== guardBeforeExpiryHold) {
    throw new Error(
      `expiry close: guard want ${guardBeforeExpiryHold} got ${guardAfterExpiry}`,
    );
  }
  log("expired close", `sig=${expiryCloseSig}\n${explorerTx(expiryCloseSig)}`);

  const summary = {
    ok: true,
    cluster: "devnet",
    finishedAt: new Date().toISOString(),
    programId: PROGRAM_ID,
    rpc: RPC_URL,
    surfpool: null,
    slotBeforeTimeTravel: Number(slotBefore),
    slotAfterTimeTravel: Number(slotAfter),
    mint: mint.address,
    destination: destination.address,
    guardToken,
    guardState,
    destBalance: (await tokenAmount(destination.address)).toString(),
    claimBalance: (await tokenAmount(claimDest.address)).toString(),
    declaredProgramId: DECLARED_PROGRAM_ID,
    usedLocalProgramId: PROGRAM_ID !== DECLARED_PROGRAM_ID,
    heldReturnData,
    before: {
      ok: true,
      kind: "ordinary-spl-tokenkeg",
      signature: beforeSig,
      mint: splMint.address,
      source: splSource.address,
      destination: splDest.address,
      amount: "50",
      destBalance: splDestBal.toString(),
      explorer: explorerTx(beforeSig),
      moneyFlow: "source → destination (always credits)",
    },
    steps: {
      credited: {
        ok: true,
        signature: creditedSig,
        explorer: explorerTx(creditedSig),
        moneyFlow: "source → destination · return 0",
      },
      held: {
        ok: true,
        signature: heldSig,
        explorer: explorerTx(heldSig),
        moneyFlow: "source → guard + receipt · return 1 · dest stays",
      },
      claim: {
        ok: true,
        signature: claimSig,
        explorer: explorerTx(claimSig),
        moneyFlow: "guard → claim destination",
      },
      expiry: {
        ok: true,
        signature: expiryCloseSig,
        holdSignature: expiryHoldSig,
        explorer: explorerTx(expiryCloseSig),
        holdExplorer: explorerTx(expiryHoldSig),
        moneyFlow: "guard → source after TTL",
      },
    },
    explorers: {
      program: `https://explorer.solana.com/address/${PROGRAM_ID}?cluster=devnet`,
      before: explorerTx(beforeSig),
      credited: explorerTx(creditedSig),
      held: explorerTx(heldSig),
      claim: explorerTx(claimSig),
      expiry: explorerTx(expiryCloseSig),
    },
    nonClaims: [
      "Custom program ID reference - not canonical Token-2022",
      "Custom mint - not legacy USDC/USDT interception",
      "Devnet demo - not mainnet product",
    ],
  };

  writeFileSync(ARTIFACT_PATH, JSON.stringify(summary, null, 2) + "\n");
  log("ok", `wrote ${ARTIFACT_PATH}\nbefore=${beforeSig}\ncredited=${creditedSig}\nheld=${heldSig}\nclaim=${claimSig}\nexpiry=${expiryCloseSig}`);
}

main().catch((err) => {
  console.error("\nFAIL", err);
  process.exit(1);
});
