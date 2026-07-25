#!/usr/bin/env bash
# Surfpool localnet checklist for receive-policy before/after demos.
# LiteSVM suite is the automated gate; this script prepares / guides Surfpool.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export PATH="${HOME}/.local/share/solana/install/active_release/bin:${HOME}/.local/bin:${PATH}"

echo "== solana-receive Surfpool before/after helper =="
echo "repo: $ROOT"
echo

if ! command -v cargo-build-sbf >/dev/null 2>&1; then
  echo "error: cargo-build-sbf not on PATH (install Agave/Solana platform tools)"
  exit 1
fi

echo "[1/4] Building SBF .so"
cargo build-sbf --manifest-path program/token-2022-receive/Cargo.toml
SO="target/deploy/token_2022_receive.so"
test -f "$SO"
echo "    ok: $SO ($(wc -c < "$SO") bytes)"
echo

echo "[2/4] Automated before/after (LiteSVM) — run this always"
cargo test -p token-2022-receive --test litesvm_before_after -- --nocapture
echo

if ! command -v surfpool >/dev/null 2>&1; then
  echo "[3/4] Surfpool CLI not found."
  echo "    Install from https://github.com/solana-foundation/surfpool/releases"
  echo "    e.g. darwin-arm64 v1.5.0 → put binary on PATH as 'surfpool'"
  echo "    Docs: docs/VERIFICATION.md"
  echo
  echo "[4/4] skipped (no surfpool)"
  exit 0
fi

echo "[3/4] Surfpool: $("$(command -v surfpool)" --version 2>/dev/null || echo present)"
echo "    Start localnet in another terminal:"
echo "      surfpool start --offline --skip-blockhash-check"
echo
echo "[4/4] Deploy checklist (after Surfpool is up on :8899):"
echo "      solana config set --url http://127.0.0.1:8899"
echo "      solana airdrop 100"
echo "      solana program deploy $SO \\"
echo "        --program-id <keypair matching declare_id! GyrTVV4h…> "
echo "      Then replay LiteSVM instruction flow via Kit client or RPC."
echo
echo "See docs/VERIFICATION.md"
