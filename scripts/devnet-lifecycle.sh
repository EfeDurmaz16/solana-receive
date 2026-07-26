#!/usr/bin/env bash
# Deployed Devnet lifecycle (ordinary SPL before + receive-policy after).
# Requires: funded ~/.config/solana/id.json, program already deployed, RECEIVE_PROGRAM_ID set
# (or matching declare_id! keypair).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export PATH="${HOME}/.local/bin:${HOME}/.local/share/solana/install/active_release/bin:${PATH}"

KP="${ROOT}/target/deploy/token_2022_receive-keypair.json"
if [[ -z "${RECEIVE_PROGRAM_ID:-}" && -f "$KP" ]]; then
  export RECEIVE_PROGRAM_ID="$(solana-keygen pubkey "$KP")"
fi
export RECEIVE_RPC_URL="${RECEIVE_RPC_URL:-https://api.devnet.solana.com}"
export RECEIVE_WS_URL="${RECEIVE_WS_URL:-wss://api.devnet.solana.com}"

echo "== solana-receive Devnet lifecycle =="
echo "program: ${RECEIVE_PROGRAM_ID:-"(declared default)"}"
echo "rpc:     $RECEIVE_RPC_URL"

rm -f demos/receive/last-run.json
(
  cd clients/js
  node --experimental-strip-types ./scripts/devnet-lifecycle.mjs
)

test -f demos/receive/last-run.json
grep -q '"ok": true' demos/receive/last-run.json
echo "Done. Artifact: demos/receive/last-run.json"
echo "Serve UI: python3 -m http.server 8765 --directory demos/receive"
