# Surfpool demo (Phase 3)

Honest local demo of the receive-policy lifecycle over **Surfpool RPC**, driven by the public Kit client in `clients/js`.

## Non-claims

- Custom program ID reference, not canonical Token-2022.
- Custom mint only, not legacy USDC/USDT interception.
- Local Surfpool evidence, not a mainnet product or ambient wallet policy.

## Pin

Surfpool CLI **v1.5.0** (hard requirement; mismatch aborts).

## Run

```bash
# once: install Surfpool v1.5.0 onto PATH (example: darwin-arm64)
curl -sL https://github.com/solana-foundation/surfpool/releases/download/v1.5.0/surfpool-darwin-arm64.tar.gz | tar xz
mv surfpool ~/.local/bin/

./scripts/surfpool-lifecycle.sh

# UI needs http (ES module + fetch); do not open file://
python3 -m http.server 8765 --directory demos/receive
# open http://127.0.0.1:8765/
```

The shell deletes `demos/receive/last-run.json` before each run. The Kit script writes a new
artifact only after credited / held / claim / expiry post-conditions pass (`ok: true`,
`finishedAt`, step signatures). A failed rerun leaves no success artifact for the UI to paint green.

If the local deploy keypair differs from `declare_id!` (`GyrTVV4h…`), the lifecycle script sets
`RECEIVE_PROGRAM_ID` automatically. Matching the declared ID requires the keypair for that pubkey.
