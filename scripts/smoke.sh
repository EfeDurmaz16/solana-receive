#!/usr/bin/env bash
# Automated smoke: build-sbf + full package tests + JS client.
# Does not run Surfpool (manual; see docs/OPERATOR.md §3).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export PATH="${HOME}/.local/share/solana/install/active_release/bin:${HOME}/.local/bin:${PATH}"

echo "== smoke: toolchain =="
command -v cargo-build-sbf >/dev/null
command -v cargo >/dev/null
command -v node >/dev/null
solana --version || true
echo

echo "== smoke: build-sbf =="
cargo build-sbf --manifest-path program/token-2022-receive/Cargo.toml
test -f target/deploy/token_2022_receive.so
echo

echo "== smoke: cargo test -p token-2022-receive =="
cargo test -p token-2022-receive
echo

echo "== smoke: clients/js =="
(
  cd clients/js
  if [[ -f package-lock.json ]]; then
    npm ci
  else
    npm install
  fi
  npm run typecheck
  npm test
)
echo

echo "== smoke: OK (automated gates) =="
echo "Next (manual): ./scripts/surfpool-lifecycle.sh"
echo "Then:          python3 -m http.server 8765 --directory demos/receive"
echo "Checklist:     docs/OPERATOR.md"
