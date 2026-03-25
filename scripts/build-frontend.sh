#!/usr/bin/env bash
#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root/crates/sp42-app"

toolchain_bin="$(rustup which cargo)"
toolchain_bin="$(dirname "$toolchain_bin")"
PATH="$toolchain_bin:$PATH" env -u NO_COLOR CLICOLOR=0 trunk build --release
