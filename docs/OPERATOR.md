# Operator path

How to re-run evidence and what the program ID story means. Read this before demos,
outreach, or grant packaging.

## Non-claims (always)

- Custom program ID reference, not canonical Token-2022 / `TokenzQd…` wire.
- Custom mint only; does not intercept legacy USDC/USDT.
- Local / reference evidence; not a mainnet product.

## Declared ID vs local deploy keypair

| Address | Meaning |
| --- | --- |
| `GyrTVV4hbcuzJuSz86FNq7K2UVAoSJQtcgHTVTz1hPPq` | `declare_id!` in the program source and the Kit client's default `PROGRAM_ID` |
| `solana-keygen pubkey target/deploy/token_2022_receive-keypair.json` | Pubkey `cargo build-sbf` will deploy to **unless** you replace that keypair |

These often **differ**. That is expected on a fresh clone.

**What works today**

1. `./scripts/surfpool-lifecycle.sh` deploys the `.so` with the **local** keypair.
2. It sets `RECEIVE_PROGRAM_ID` to that pubkey for the Kit script.
3. Every builder and PDA finder in the lifecycle uses that override. Evidence is valid for the
   deployed program, not a silent test of the declared ID.

**What is not claimed**

- A green Surfpool run does **not** prove you can deploy at `GyrTVV4h…` without the matching
  secret keypair.
- Never commit `*-keypair.json`.

**If you need the declared ID on a localnet**

1. Obtain or grind a keypair whose pubkey is exactly `GyrTVV4hbcuzJuSz86FNq7K2UVAoSJQtcgHTVTz1hPPq`.
2. Replace `target/deploy/token_2022_receive-keypair.json` with it (or pass it to
   `solana program deploy --program-id`).
3. Rebuild / redeploy so the on-chain executable and the client default ID agree.
4. Then `RECEIVE_PROGRAM_ID` can be omitted and the default Kit address matches the chain.

Until step 1 exists in your custody, treat declared ID as the **published identity** of the
reference and local keypair deploys as **fidelity evidence** under an override.

## Smoke checklist

Run from the repo root. Stop on first failure.

### 0. Toolchain

```bash
export PATH="$HOME/.local/share/solana/install/active_release/bin:$HOME/.local/bin:$PATH"
solana --version          # Agave 4.1.x when CU table was measured
rustc --version
node --version            # 22+ recommended for strip-types client tests
```

### 1. Automated semantic gate (required)

```bash
cargo build-sbf --manifest-path program/token-2022-receive/Cargo.toml
cargo test -p token-2022-receive
cd clients/js && npm ci && npm run typecheck && npm test && cd ../..
```

Or the wrapper:

```bash
./scripts/smoke.sh
```

Expect: all Rust tests pass; JS reports 14 passing (or current count in CI).

### 2. LiteSVM lifecycle + CU (required for CU claims)

```bash
cargo test -p token-2022-receive --test litesvm_before_after --test litesvm_lifecycle -- --nocapture
cargo test -p token-2022-receive --test cu_ceilings -- --nocapture
```

CU numbers live in [VERIFICATION.md](./VERIFICATION.md). Do not cite Surfpool for CU.

### 3. Surfpool + Kit RPC (manual fidelity)

Pin: Surfpool CLI **exactly** `surfpool 1.5.0` on `PATH`.

```bash
./scripts/surfpool-lifecycle.sh
```

Pass only if:

- Script exits 0
- `demos/receive/last-run.json` exists with `"ok": true`, `finishedAt`, and
  `steps.{credited,held,claim,expiry}.signature`
- `usedLocalProgramId` is honest relative to the deployed pubkey vs declared ID

Then serve the UI (module + fetch need http):

```bash
python3 -m http.server 8765 --directory demos/receive
# open http://127.0.0.1:8765/
```

The UI must show a successful evidence banner with `finishedAt`. A missing or non-`ok`
artifact must **not** paint a full green lifecycle.

### 4. Defend in one minute

| Question | Answer |
| --- | --- |
| What did we prove? | Held delivery semantics on a Token-2022-**shaped** custom program, plus Kit assembly and offline Surfpool RPC |
| CU? | LiteSVM table in VERIFICATION; held costs more than no-policy; under default budget |
| TokenzQd / USDC? | No |
| Declared ID deployed? | Only if keypair matches; Surfpool script usually overrides |

## Related

- [VERIFICATION.md](./VERIFICATION.md) — suites, CU table, Surfpool section
- [WIRE.md](./WIRE.md) — frozen byte/account contract
- [SPEC.md](./SPEC.md) — normative behavior
- [demos/receive/README.md](../demos/receive/README.md) — demo UI notes
- [proposals/maintainer-discussion.md](./proposals/maintainer-discussion.md) — outreach draft (do not post cold without OK)
