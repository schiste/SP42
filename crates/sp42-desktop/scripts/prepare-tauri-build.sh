#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
desktop_dir="$(cd "$script_dir/.." && pwd)"
repo_root="$(cd "$desktop_dir/../.." && pwd)"

mode="release"
target_triple="${SP42_TAURI_TARGET_TRIPLE:-}"
locked=0
frozen=0
offline=0

usage() {
  cat <<'EOF'
Usage: crates/sp42-desktop/scripts/prepare-tauri-build.sh [--release|--dev] [--target TRIPLE] [--locked|--unlocked] [--frozen] [--offline]

Builds the SP42 browser bundle and prepares the sp42-server Tauri sidecar binary.
EOF
}

while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --release)
      mode="release"
      shift
      ;;
    --dev|--debug)
      mode="dev"
      shift
      ;;
    --target)
      target_triple="${2:?--target requires a value}"
      shift 2
      ;;
    --locked)
      locked=1
      shift
      ;;
    --unlocked)
      locked=0
      shift
      ;;
    --frozen)
      frozen=1
      shift
      ;;
    --offline)
      offline=1
      shift
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "Unsupported option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

source "$repo_root/scripts/lib/build-common.sh"
sp42_setup_build_env "$repo_root" "$mode"

cargo_bin="$(sp42_cargo_bin)"
host_triple="$("$cargo_bin" -vV | sed -n 's/^host: //p')"
target_triple="${target_triple:-$host_triple}"

server_args=(build -p sp42-server)
if [[ "$mode" == "release" ]]; then
  server_args+=(--release)
fi
if [[ "$target_triple" != "$host_triple" ]]; then
  server_args+=(--target "$target_triple")
fi
if [[ "$locked" == "1" ]]; then
  server_args+=(--locked)
fi
if [[ "$frozen" == "1" ]]; then
  server_args+=(--frozen)
fi
if [[ "$offline" == "1" ]]; then
  server_args+=(--offline)
fi

build_flags=()
if [[ "$locked" == "1" ]]; then
  build_flags+=(--locked)
else
  build_flags+=(--unlocked)
fi
if [[ "$frozen" == "1" ]]; then
  build_flags+=(--frozen)
fi
if [[ "$offline" == "1" ]]; then
  build_flags+=(--offline)
fi

if [[ "$mode" == "release" ]]; then
  "$repo_root/scripts/build-web-release.sh" "${build_flags[@]}"
else
  "$repo_root/scripts/build-frontend.sh" --debug "${build_flags[@]}"
fi
"$cargo_bin" "${server_args[@]}"

binary_ext=""
if [[ "$target_triple" == *"windows"* ]]; then
  binary_ext=".exe"
fi

profile_dir="debug"
if [[ "$mode" == "release" ]]; then
  profile_dir="release"
fi

target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"
source_binary="$target_dir/$profile_dir/sp42-server$binary_ext"
if [[ "$target_triple" != "$host_triple" ]]; then
  source_binary="$target_dir/$target_triple/$profile_dir/sp42-server$binary_ext"
fi
if [[ ! -f "$source_binary" ]]; then
  printf 'Expected sidecar source binary was not built: %s\n' "$source_binary" >&2
  exit 1
fi

sidecar_dir="$repo_root/crates/sp42-desktop/src-tauri/binaries"
sidecar_binary="$sidecar_dir/sp42-server-$target_triple$binary_ext"

mkdir -p "$sidecar_dir"
cp "$source_binary" "$sidecar_binary"
chmod +x "$sidecar_binary" 2>/dev/null || true

printf 'Prepared Tauri sidecar: %s\n' "$sidecar_binary"
