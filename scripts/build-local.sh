#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

scripts/clean-house.sh

cargo_bin="${CARGO_BIN:-$(command -v cargo)}"
workspace_target_dir="${CARGO_TARGET_DIR_WORKSPACE:-$repo_root/target/workspace}"
tauri_target_dir="${CARGO_TARGET_DIR_TAURI:-$repo_root/target/tauri}"
mkdir -p "$workspace_target_dir" "$tauri_target_dir"

CARGO_TARGET_DIR="$workspace_target_dir" "$cargo_bin" build --workspace --all-targets
CARGO_TARGET_DIR="$workspace_target_dir" "$cargo_bin" build -p sp42-core --target wasm32-unknown-unknown
CARGO_TARGET_DIR="$workspace_target_dir" "$cargo_bin" build -p sp42-app --target wasm32-unknown-unknown
CARGO_TARGET_DIR="$tauri_target_dir" "$cargo_bin" build --manifest-path crates/sp42-desktop/src-tauri/Cargo.toml

printf 'SP42 local build complete.\n'
