#!/usr/bin/env bash
# Local semantic smoke aligned with CI automated gates (except Surfpool).
# Does not run Surfpool (manual; see docs/OPERATOR.md §3).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export PATH="${HOME}/.local/share/solana/install/active_release/bin:${HOME}/.local/bin:${PATH}"

echo "== smoke: toolchain =="
command -v cargo-build-sbf >/dev/null
command -v cargo >/dev/null
command -v node >/dev/null
command -v solana >/dev/null
solana --version
echo

echo "== smoke: fmt + lockfile + clippy (lib) =="
cargo fmt --all -- --check
cargo metadata --locked --format-version 1 >/dev/null
cargo clippy -p token-2022-receive --lib -- -D warnings
echo

echo "== smoke: build-sbf (locked) =="
cargo build-sbf --manifest-path program/token-2022-receive/Cargo.toml -- --locked
test -f target/deploy/token_2022_receive.so
echo

echo "== smoke: cargo test -p token-2022-receive (locked) =="
cargo test -p token-2022-receive --locked
echo

echo "== smoke: Codama codegen freshness =="
if [[ ! -f package-lock.json ]]; then
  echo "missing root package-lock.json" >&2
  exit 1
fi
npm ci
npm run codegen:check
echo

echo "== smoke: clients/js =="
(
  cd clients/js
  if [[ ! -f package-lock.json ]]; then
    echo "missing clients/js/package-lock.json" >&2
    exit 1
  fi
  npm ci
  npm run typecheck
  npm test
  node --experimental-strip-types \
    -e "import('./src/index.ts').then(m => { if (Object.keys(m).length < 50) { throw new Error('entry exported ' + Object.keys(m).length + ' names'); } console.log('entry OK,', Object.keys(m).length, 'exports'); })"
)
echo

echo "== smoke: OK (CI-aligned automated gates; Surfpool still manual) =="
echo "Next (manual): ./scripts/surfpool-lifecycle.sh"
echo "Then:          python3 -m http.server 8765 --directory demos/receive"
echo "Checklist:     docs/OPERATOR.md"
