#!/usr/bin/env bash
# Surfpool localnet: build → start → deploy → Kit lifecycle (credited/held/claim/expiry).
#
# Pin: Surfpool CLI v1.5.0
#   darwin-arm64: https://github.com/solana-foundation/surfpool/releases/download/v1.5.0/surfpool-darwin-arm64.tar.gz
# LiteSVM remains the automated semantic/CU gate; this is RPC fidelity + demo evidence.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export PATH="${HOME}/.local/bin:${HOME}/.local/share/solana/install/active_release/bin:${PATH}"

SURFPOOL_PIN="1.5.0"
RPC_URL="${RECEIVE_RPC_URL:-http://127.0.0.1:8899}"
WS_URL="${RECEIVE_WS_URL:-ws://127.0.0.1:8900}"
RPC_PORT="${RECEIVE_RPC_PORT:-8899}"
WS_PORT="${RECEIVE_WS_PORT:-8900}"
SURFPOOL_PID=""

cleanup() {
  if [[ -n "${SURFPOOL_PID}" ]] && kill -0 "${SURFPOOL_PID}" 2>/dev/null; then
    kill "${SURFPOOL_PID}" 2>/dev/null || true
    wait "${SURFPOOL_PID}" 2>/dev/null || true
  fi
}
trap cleanup EXIT

echo "== solana-receive Surfpool lifecycle =="
echo "repo: $ROOT"
echo "surfpool pin: v${SURFPOOL_PIN}"
echo

if ! command -v cargo-build-sbf >/dev/null 2>&1; then
  echo "error: cargo-build-sbf not on PATH"
  exit 1
fi

if ! command -v surfpool >/dev/null 2>&1; then
  echo "error: surfpool not on PATH"
  echo "Install v${SURFPOOL_PIN} from https://github.com/solana-foundation/surfpool/releases/tag/v${SURFPOOL_PIN}"
  exit 1
fi

VERSION_LINE="$(surfpool --version 2>/dev/null || true)"
echo "surfpool: ${VERSION_LINE}"
if [[ "${VERSION_LINE}" != *"${SURFPOOL_PIN}"* ]]; then
  echo "warning: expected Surfpool v${SURFPOOL_PIN}; continuing with whatever is installed"
fi

echo "[1/5] Building SBF .so"
cargo build-sbf --manifest-path program/token-2022-receive/Cargo.toml
SO="target/deploy/token_2022_receive.so"
KP="target/deploy/token_2022_receive-keypair.json"
test -f "$SO"
test -f "$KP"
PROGRAM_ID="$(solana-keygen pubkey "$KP")"
DECLARED_ID="GyrTVV4hbcuzJuSz86FNq7K2UVAoSJQtcgHTVTz1hPPq"
echo "    so: $SO ($(wc -c < "$SO") bytes)"
echo "    deploy program id: $PROGRAM_ID"
if [[ "$PROGRAM_ID" != "$DECLARED_ID" ]]; then
  echo "    note: local keypair differs from declare_id! (${DECLARED_ID})"
  echo "          lifecycle script will use RECEIVE_PROGRAM_ID=${PROGRAM_ID}"
fi
echo

echo "[2/5] Starting Surfpool (offline, no auto-deploy, CI mode)"
surfpool start \
  --offline \
  --skip-blockhash-check \
  --no-deploy \
  --ci \
  --port "${RPC_PORT}" \
  --ws-port "${WS_PORT}" \
  >/tmp/solana-receive-surfpool.log 2>&1 &
SURFPOOL_PID=$!

for i in $(seq 1 60); do
  if curl -sf "$RPC_URL" -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getHealth","params":[]}' >/dev/null 2>&1 \
    || curl -sf "$RPC_URL" -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"getVersion","params":[]}' >/dev/null 2>&1; then
    echo "    rpc ready at $RPC_URL"
    break
  fi
  if ! kill -0 "${SURFPOOL_PID}" 2>/dev/null; then
    echo "error: surfpool exited early; see /tmp/solana-receive-surfpool.log"
    cat /tmp/solana-receive-surfpool.log | tail -40
    exit 1
  fi
  if [[ "$i" -eq 60 ]]; then
    echo "error: rpc not ready; see /tmp/solana-receive-surfpool.log"
    tail -40 /tmp/solana-receive-surfpool.log
    exit 1
  fi
  sleep 0.5
done
echo

echo "[3/5] Deploy program"
solana config set --url "$RPC_URL" >/dev/null
# Ensure default keypair can pay fees.
if [[ ! -f "${HOME}/.config/solana/id.json" ]]; then
  solana-keygen new --no-bip39-passphrase --silent -o "${HOME}/.config/solana/id.json"
fi
solana airdrop 100 >/dev/null
solana program deploy "$SO" --program-id "$KP"
echo "    deployed $PROGRAM_ID"
echo

echo "[4/5] Kit client lifecycle (credited → held → claim / expiry)"
(
  cd clients/js
  RECEIVE_RPC_URL="$RPC_URL" \
  RECEIVE_WS_URL="$WS_URL" \
  RECEIVE_PROGRAM_ID="$PROGRAM_ID" \
  node --experimental-strip-types ./scripts/surfpool-lifecycle.mjs
)
echo

echo "[5/5] Done"
echo "    demo snapshot: demos/receive/last-run.json"
echo "    open demos/receive/index.html for the honest walkthrough UI"
echo "    Surfpool log: /tmp/solana-receive-surfpool.log"
