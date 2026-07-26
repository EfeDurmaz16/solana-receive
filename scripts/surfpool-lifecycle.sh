#!/usr/bin/env bash
# Surfpool localnet: build → start → deploy → Kit lifecycle (credited/held/claim/expiry).
#
# Pin: Surfpool CLI v1.5.0 (hard requirement; mismatch aborts).
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
ARTIFACT="${ROOT}/demos/receive/last-run.json"
SURFPOOL_PID=""
DEPLOY_TIMEOUT_SEC="${RECEIVE_DEPLOY_TIMEOUT_SEC:-120}"
LIFECYCLE_TIMEOUT_SEC="${RECEIVE_LIFECYCLE_TIMEOUT_SEC:-180}"

cleanup() {
  if [[ -n "${SURFPOOL_PID}" ]] && kill -0 "${SURFPOOL_PID}" 2>/dev/null; then
    kill "${SURFPOOL_PID}" 2>/dev/null || true
    wait "${SURFPOOL_PID}" 2>/dev/null || true
  fi
}
trap cleanup EXIT

run_with_timeout() {
  local secs="$1"
  shift
  if command -v timeout >/dev/null 2>&1; then
    timeout "${secs}" "$@"
  elif command -v gtimeout >/dev/null 2>&1; then
    gtimeout "${secs}" "$@"
  else
    "$@"
  fi
}

echo "== solana-receive Surfpool lifecycle =="
echo "repo: $ROOT"
echo "surfpool pin: v${SURFPOOL_PIN}"
echo

# Stale success artifact must not survive a failed rerun (demo UI evidence).
rm -f "${ARTIFACT}"
mkdir -p "$(dirname "${ARTIFACT}")"

if ! command -v cargo-build-sbf >/dev/null 2>&1; then
  echo "error: cargo-build-sbf not on PATH"
  exit 1
fi

if ! command -v surfpool >/dev/null 2>&1; then
  echo "error: surfpool not on PATH"
  echo "Install v${SURFPOOL_PIN} from https://github.com/solana-foundation/surfpool/releases/tag/v${SURFPOOL_PIN}"
  exit 1
fi

VERSION_LINE="$(surfpool --version 2>/dev/null | head -n1 | tr -d '\r')"
echo "surfpool: ${VERSION_LINE}"
# Exact match only (substring would accept "surfpool 11.5.0" for pin 1.5.0).
if [[ "${VERSION_LINE}" != "surfpool ${SURFPOOL_PIN}" ]]; then
  echo "error: expected exact 'surfpool ${SURFPOOL_PIN}', got: '${VERSION_LINE}'"
  echo "Install the pin from https://github.com/solana-foundation/surfpool/releases/tag/v${SURFPOOL_PIN}"
  exit 1
fi

# True when the TCP listener on RPC_PORT is our Surfpool child (or a descendant).
rpc_listener_is_ours() {
  local listen_pid=""
  listen_pid="$(lsof -nP -iTCP:"${RPC_PORT}" -sTCP:LISTEN -t 2>/dev/null | head -n1 || true)"
  if [[ -z "${listen_pid}" ]]; then
    return 1
  fi
  local p="${listen_pid}"
  local i=0
  while [[ -n "${p}" && "${p}" != "0" && "${p}" != "1" && "${i}" -lt 16 ]]; do
    if [[ "${p}" == "${SURFPOOL_PID}" ]]; then
      return 0
    fi
    p="$(ps -o ppid= -p "${p}" 2>/dev/null | tr -d ' ' || true)"
    i=$((i + 1))
  done
  return 1
}

# Surfpool-only cheatcode; ordinary solana-test-validator / foreign RPC returns method-not-found.
# Pause then immediately resume so we do not freeze the simnet for deploy/lifecycle.
rpc_is_surfpool() {
  local pause_body resume_body
  pause_body="$(curl -sf "$RPC_URL" -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","id":1,"method":"surfnet_pauseClock","params":[]}' 2>/dev/null || true)"
  if [[ "${pause_body}" != *"\"result\""* ]]; then
    return 1
  fi
  resume_body="$(curl -sf "$RPC_URL" -H 'content-type: application/json' \
    -d '{"jsonrpc":"2.0","id":2,"method":"surfnet_resumeClock","params":[]}' 2>/dev/null || true)"
  [[ "${resume_body}" == *"\"result\""* ]]
}

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
  # Require our child to still be alive before trusting anything on the port.
  if ! kill -0 "${SURFPOOL_PID}" 2>/dev/null; then
    echo "error: surfpool exited early; see /tmp/solana-receive-surfpool.log"
    tail -40 /tmp/solana-receive-surfpool.log
    exit 1
  fi
  # Bind ownership + Surfpool fingerprint: reject a foreign process answering on this port
  # even while our child PID is still alive (failed bind / race).
  if rpc_listener_is_ours && rpc_is_surfpool; then
    echo "    rpc ready at $RPC_URL (pid ${SURFPOOL_PID}, listener owned + surfnet cheatcode ok)"
    break
  fi
  if [[ "$i" -eq 60 ]]; then
    echo "error: rpc not ready as our Surfpool; see /tmp/solana-receive-surfpool.log"
    echo "    tip: another process may already own port ${RPC_PORT}"
    tail -40 /tmp/solana-receive-surfpool.log
    exit 1
  fi
  sleep 0.5
done
echo

echo "[3/5] Deploy program"
# Do not mutate global ~/.config/solana/cli/config.yml; pass --url per command.
if [[ ! -f "${HOME}/.config/solana/id.json" ]]; then
  solana-keygen new --no-bip39-passphrase --silent -o "${HOME}/.config/solana/id.json"
fi
run_with_timeout "${DEPLOY_TIMEOUT_SEC}" solana airdrop 100 --url "$RPC_URL" >/dev/null
run_with_timeout "${DEPLOY_TIMEOUT_SEC}" solana program deploy "$SO" --program-id "$KP" --url "$RPC_URL"
echo "    deployed $PROGRAM_ID"
echo

echo "[4/5] Kit client lifecycle (credited → held → claim / expiry)"
(
  cd clients/js
  export RECEIVE_RPC_URL="$RPC_URL"
  export RECEIVE_WS_URL="$WS_URL"
  export RECEIVE_PROGRAM_ID="$PROGRAM_ID"
  export RECEIVE_SURFPOOL_VERSION="${SURFPOOL_PIN}"
  run_with_timeout "${LIFECYCLE_TIMEOUT_SEC}" \
    node --experimental-strip-types ./scripts/surfpool-lifecycle.mjs
)
echo

if [[ ! -f "${ARTIFACT}" ]]; then
  echo "error: lifecycle finished without writing ${ARTIFACT}"
  exit 1
fi
if ! grep -q '"ok": true' "${ARTIFACT}"; then
  echo "error: ${ARTIFACT} is not a successful evidence artifact"
  exit 1
fi

echo "[5/5] Done"
echo "    demo snapshot: ${ARTIFACT}"
echo "    serve the UI (module + fetch need http):"
echo "      python3 -m http.server 8765 --directory demos/receive"
echo "      open http://127.0.0.1:8765/"
echo "    Surfpool log: /tmp/solana-receive-surfpool.log"
