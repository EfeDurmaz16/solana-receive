#!/usr/bin/env node
/**
 * Phase 3: credited → held → claim / expiry over Surfpool RPC.
 *
 * Uses only the public Kit client package (generated builders + residual helpers).
 * Requires a running Surfpool (`surfpool start --offline --skip-blockhash-check --no-deploy --ci`)
 * with the reference program deployed.
 *
 * Program ID: defaults to the declared ID. When the local build-sbf keypair differs
 * (common), pass RECEIVE_PROGRAM_ID=<deployed pubkey>.
 *
 * Pin: Surfpool CLI v1.5.0 (see docs/VERIFICATION.md).
 */
import { unlinkSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import {
  AccountRole,
  address,
  appendTransactionMessageInstructions,
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

const RPC_URL = process.env.RECEIVE_RPC_URL ?? "http://127.0.0.1:8899";
const WS_URL = process.env.RECEIVE_WS_URL ?? "ws://127.0.0.1:8900";
const PROGRAM_ID = address(process.env.RECEIVE_PROGRAM_ID ?? DECLARED_PROGRAM_ID);
const OBSERVED_SURFPOOL_VERSION = process.env.RECEIVE_SURFPOOL_VERSION ?? null;
const SYSTEM_PROGRAM = address("11111111111111111111111111111111");
const DECIMALS = 6;
const MIN_AMOUNT = 100n;
/** Short TTL so expiry can be demonstrated via surfnet_timeTravel. */
const DEMO_TTL_SLOTS = 32n;
const BOND_LAMPORTS = 1_000_000n;
const MINT_SIZE = 82;
const ACCOUNT_SIZE = 165;
/** ACCOUNT_SIZE + account-type + TLV header + ReceivePolicy (see docs/WIRE.md). */
const ACCOUNT_WITH_POLICY_SIZE = 498;
const ARTIFACT_PATH = resolve(__dirname, "../../../demos/receive/last-run.json");

const rpc = createSolanaRpc(RPC_URL);
const rpcSubscriptions = createSolanaRpcSubscriptions(WS_URL);
const sendAndConfirm = sendAndConfirmTransactionFactory({ rpc, rpcSubscriptions });
const programConfig = { programAddress: PROGRAM_ID };

/** Remove any prior success artifact before this run can claim green. */
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

async function rpcCall(method, params = []) {
  const res = await fetch(RPC_URL, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }),
  });
  const body = await res.json();
  if (body.error) {
    throw new Error(`${method}: ${JSON.stringify(body.error)}`);
  }
  return body.result;
}

async function airdrop(to, lamports = 10_000_000_000n) {
  const sig = await rpc.requestAirdrop(to, lamports).send();
  // Surfpool airdrops land quickly; poll signature status briefly.
  for (let i = 0; i < 40; i++) {
    const st = await rpc.getSignatureStatuses([sig], { searchTransactionHistory: true }).send();
    const v = st.value[0];
    if (v?.confirmationStatus === "confirmed" || v?.confirmationStatus === "finalized") return;
    if (v?.err) throw new Error(`airdrop failed: ${JSON.stringify(v.err)}`);
    await new Promise((r) => setTimeout(r, 100));
  }
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

async function main() {
  clearEvidenceArtifact();
  log("surfpool lifecycle", `rpc=${RPC_URL}\nprogram=${PROGRAM_ID}`);

  // Health check.
  const health = await rpcCall("getHealth").catch(() => null);
  if (health !== "ok") {
    // Some Surfpool builds return null/omit; fall back to getVersion.
    await rpc.getVersion().send();
  }

  const payer = await generateKeyPairSigner();
  const mintAuthority = await generateKeyPairSigner();
  const sourceOwner = await generateKeyPairSigner();
  const destOwner = await generateKeyPairSigner();
  const mint = await generateKeyPairSigner();
  const source = await generateKeyPairSigner();
  const destination = await generateKeyPairSigner();
  const claimDest = await generateKeyPairSigner();

  await airdrop(payer.address);
  log("funded payer", payer.address);

  const mintRent = await rpc.getMinimumBalanceForRentExemption(BigInt(MINT_SIZE)).send();
  const accountRent = await rpc.getMinimumBalanceForRentExemption(BigInt(ACCOUNT_SIZE)).send();
  const policyRent = await rpc
    .getMinimumBalanceForRentExemption(BigInt(ACCOUNT_WITH_POLICY_SIZE))
    .send();
  const receiptRent = await rpc.getMinimumBalanceForRentExemption(BigInt(RECEIPT_SIZE)).send();

  // 1) Mint + source token account + mint_to.
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

  await sendIx(
    "mint_to source",
    payer,
    [
      getMintToInstruction(
        {
          mint: mint.address,
          account: source.address,
          mintAuthority,
          amount: 1_000_000n,
        },
        programConfig,
      ),
    ],
  );
  log("source funded", `amount=${await tokenAmount(source.address)}`);

  // 2) Policy destination + EnsureGuard.
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
        recoveryAuthorityMode: 0, // Originator
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

  // 3a) Credited path (amount >= min).
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
  log("credited", `sig=${creditedSig}\ndest=${destAfterCredit}`);

  // 3b) Held path (amount < min).
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
  log("held", `sig=${heldSig}\nguard=${guardAfterHold}\nreceipt=${heldReceipt}`);

  // Return data is last-ix scoped; fetch tx meta when available.
  // Balance pins above remain authoritative; wrong return data still fails the run.
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
    } else {
      log("held return data", "unavailable on this RPC build (balance check is authoritative)");
    }
  } catch (e) {
    const msg = String(e?.message ?? e);
    if (msg.startsWith("return data want") || msg.startsWith("unrecognized transfer outcome")) {
      throw e;
    }
    log("held return data", `skip: ${msg}`);
  }

  // 4a) Claim (Originator = sourceOwner).
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
  log("claimed", `sig=${claimSig}\nclaimDest=${claimBal}`);

  // 4b) Second hold + expiry close after time travel.
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

  const slotBefore = await rpc.getSlot().send();
  const minExpiredSlot = BigInt(slotBefore) + DEMO_TTL_SLOTS + 1n;
  const targetSlot = minExpiredSlot + 10n;
  await rpcCall("surfnet_timeTravel", [{ absoluteSlot: Number(targetSlot) }]);
  let slotAfter = await rpc.getSlot().send();
  // Some Surfpool builds land one short of absoluteSlot; nudge once if still pre-expiry.
  if (BigInt(slotAfter) < minExpiredSlot) {
    await rpcCall("surfnet_timeTravel", [{ absoluteSlot: Number(targetSlot + 20n) }]);
    slotAfter = await rpc.getSlot().send();
  }
  if (BigInt(slotAfter) < minExpiredSlot) {
    throw new Error(
      `time travel: slot want >= ${minExpiredSlot} (ttl+1) got ${slotAfter}`,
    );
  }
  log("time travel", `from=${slotBefore} to=${slotAfter} (minExpired ${minExpiredSlot})`);

  // Tokens return to a source-owner token account (reuse `source`).
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
    // Hold debited source by 1; close should restore that 1.
    throw new Error(
      `expiry close: source want ${sourceBeforeExpiryHold} got ${sourceAfterExpiry}`,
    );
  }
  if (guardAfterExpiry !== guardBeforeExpiryHold) {
    throw new Error(
      `expiry close: guard want ${guardBeforeExpiryHold} got ${guardAfterExpiry}`,
    );
  }
  log("expired close", `sig=${expiryCloseSig}\nsource=${sourceAfterExpiry}\nguard=${guardAfterExpiry}`);

  if (!OBSERVED_SURFPOOL_VERSION) {
    throw new Error(
      "RECEIVE_SURFPOOL_VERSION unset; refuse to write evidence that invents a Surfpool pin",
    );
  }

  const summary = {
    ok: true,
    finishedAt: new Date().toISOString(),
    programId: PROGRAM_ID,
    rpc: RPC_URL,
    surfpool: OBSERVED_SURFPOOL_VERSION,
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
    steps: {
      credited: { ok: true, signature: creditedSig },
      held: { ok: true, signature: heldSig },
      claim: { ok: true, signature: claimSig },
      expiry: {
        ok: true,
        signature: expiryCloseSig,
        holdSignature: expiryHoldSig,
      },
    },
    nonClaims: [
      "Custom program ID reference - not canonical Token-2022",
      "Custom mint - not legacy USDC/USDT interception",
      "Local Surfpool demo - not mainnet product",
    ],
  };

  writeFileSync(ARTIFACT_PATH, JSON.stringify(summary, null, 2) + "\n");
  log("ok", `wrote ${ARTIFACT_PATH}\n${JSON.stringify(summary, null, 2)}`);
}

main().catch((err) => {
  console.error("\nFAIL", err);
  process.exit(1);
});
