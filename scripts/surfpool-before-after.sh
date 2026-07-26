#!/usr/bin/env bash
# Thin wrapper kept for older docs/links. Prefer ./scripts/surfpool-lifecycle.sh.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
exec "$ROOT/scripts/surfpool-lifecycle.sh" "$@"
