# Surfpool demo (Phase 3)

Honest local demo of the receive-policy lifecycle over **Surfpool RPC**, driven by the public Kit client in `clients/js`.

## Non-claims

- Custom program ID reference, not canonical Token-2022.
- Custom mint only, not legacy USDC/USDT interception.
- Local Surfpool evidence, not a mainnet product or ambient wallet policy.

## Pin

Surfpool CLI **v1.5.0**.

## Run

```bash
# once: install Surfpool v1.5.0 onto PATH (example: darwin-arm64)
curl -sL https://github.com/solana-foundation/surfpool/releases/download/v1.5.0/surfpool-darwin-arm64.tar.gz | tar xz
mv surfpool ~/.local/bin/

./scripts/surfpool-lifecycle.sh
```

That script builds the `.so`, starts Surfpool offline, deploys with the local build-sbf keypair, and runs `scripts/surfpool-lifecycle.mjs` (credited → held → claim → expiry via `surfnet_timeTravel`).

Open `demos/receive/index.html` after a successful run to load `last-run.json`.

If the local deploy keypair differs from `declare_id!` (`GyrTVV4h…`), the lifecycle script sets `RECEIVE_PROGRAM_ID` automatically. Matching the declared ID requires the keypair for that pubkey.
