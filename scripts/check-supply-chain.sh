#!/usr/bin/env bash
# Supply-chain gate: cargo-deny (advisories + licenses + bans + sources) and
# cargo-audit (RustSec advisory DB). Both are BLOCKING and run with no ignores
# beyond what deny.toml declares (project policy: fully strict).
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

need() {
  command -v "$1" >/dev/null 2>&1 || {
    printf 'SP42 supply-chain check: `%s` is not installed.\n  install: %s\n' "$1" "$2" >&2
    exit 1
  }
}
need cargo-deny "cargo install --locked cargo-deny"
need cargo-audit "cargo install --locked cargo-audit"

status=0

printf '\n== cargo deny check ==\n'
cargo deny check || status=1

printf '\n== cargo audit ==\n'
cargo audit || status=1

if (( status != 0 )); then
  printf '\nSP42 supply-chain check FAILED.\n' >&2
  exit 1
fi
printf '\nSP42 supply-chain check passed.\n'
