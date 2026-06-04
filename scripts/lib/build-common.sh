#!/usr/bin/env bash

sp42_repo_root() {
  local script_dir
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
  printf '%s\n' "$script_dir"
}

sp42_cargo_bin() {
  if [[ -n "${CARGO_BIN:-}" ]]; then
    printf '%s\n' "$CARGO_BIN"
    return 0
  fi

  if command -v rustup >/dev/null 2>&1; then
    local rustup_cargo
    rustup_cargo="$(rustup which cargo 2>/dev/null || true)"
    if [[ -n "$rustup_cargo" ]]; then
      printf '%s\n' "$rustup_cargo"
      return 0
    fi
  fi

  command -v cargo
}

sp42_cpu_count() {
  if command -v getconf >/dev/null 2>&1; then
    getconf _NPROCESSORS_ONLN 2>/dev/null && return 0
  fi

  if command -v sysctl >/dev/null 2>&1; then
    sysctl -n hw.logicalcpu 2>/dev/null && return 0
  fi

  printf '4\n'
}

sp42_source_date_epoch() {
  local repo_root="$1"

  if command -v git >/dev/null 2>&1; then
    git -C "$repo_root" log -1 --format=%ct 2>/dev/null && return 0
  fi

  printf '0\n'
}

sp42_append_flag() {
  local current_flags="$1"
  local new_flag="$2"

  if [[ -z "$current_flags" ]]; then
    printf '%s\n' "$new_flag"
    return 0
  fi

  case " $current_flags " in
    *" $new_flag "*) printf '%s\n' "$current_flags" ;;
    *) printf '%s %s\n' "$current_flags" "$new_flag" ;;
  esac
}

sp42_frontend_dist_dir() {
  local repo_root="$1"
  printf '%s/target/dist/sp42-app\n' "$repo_root"
}

sp42_prepend_cargo_path() {
  local cargo_bin cargo_dir
  cargo_bin="$(sp42_cargo_bin)"
  cargo_dir="$(cd "$(dirname "$cargo_bin")" && pwd)"

  case ":$PATH:" in
    *":$cargo_dir:"*) ;;
    *) export PATH="$cargo_dir:$PATH" ;;
  esac
}

sp42_clean_build_slate() {
  local repo_root="$1"

  if [[ "${SP42_BUILD_SLATE_CLEANED:-0}" == "1" ]]; then
    return 0
  fi

  "$repo_root/scripts/clean-house.sh" --purge-target
  export SP42_BUILD_SLATE_CLEANED=1
}

sp42_run_xtask() {
  local repo_root="$1"
  local task="$2"
  shift 2

  local cargo_bin
  cargo_bin="$(sp42_cargo_bin)"
  local cargo_flags=(-q -p xtask)

  for arg in "$@"; do
    case "$arg" in
      --locked)
        cargo_flags+=(--locked)
        ;;
      --frozen)
        cargo_flags+=(--frozen)
        ;;
      --offline)
        cargo_flags+=(--offline)
        ;;
    esac
  done

  CARGO_BIN="$cargo_bin" "$cargo_bin" run "${cargo_flags[@]}" -- "$task" "$@"
}

sp42_maybe_enable_sccache() {
  local requested="${SP42_USE_SCCACHE:-auto}"

  case "$requested" in
    auto)
      ;;
    1|true|TRUE|yes|YES)
      ;;
    0|false|FALSE|no|NO)
      return 0
      ;;
    *)
      printf 'Invalid SP42_USE_SCCACHE value: %s\n' "$requested" >&2
      return 1
      ;;
  esac

  if ! command -v sccache >/dev/null 2>&1; then
    if [[ "$requested" != "auto" ]]; then
      printf 'SP42_USE_SCCACHE is enabled but `sccache` is not installed.\n' >&2
      return 1
    fi
    return 0
  fi

  export RUSTC_WRAPPER="${RUSTC_WRAPPER:-$(command -v sccache)}"
  export SCCACHE_IDLE_TIMEOUT="${SCCACHE_IDLE_TIMEOUT:-0}"
}

sp42_setup_build_env() {
  local repo_root="$1"
  local mode="${2:-dev}"

  export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$repo_root/target}"
  export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-$(sp42_cpu_count)}"
  export CLICOLOR=0
  unset NO_COLOR || true

  if command -v rustup >/dev/null 2>&1; then
    local rustc_bin rustdoc_bin
    rustc_bin="$(rustup which rustc 2>/dev/null || true)"
    rustdoc_bin="$(rustup which rustdoc 2>/dev/null || true)"
    if [[ -n "$rustc_bin" ]]; then
      export RUSTC="${RUSTC:-$rustc_bin}"
    fi
    if [[ -n "$rustdoc_bin" ]]; then
      export RUSTDOC="${RUSTDOC:-$rustdoc_bin}"
    fi
  fi

  sp42_prepend_cargo_path
  sp42_maybe_enable_sccache

  case "$mode" in
    release)
      export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
      export SOURCE_DATE_EPOCH="${SOURCE_DATE_EPOCH:-$(sp42_source_date_epoch "$repo_root")}"
      export RUSTFLAGS="$(sp42_append_flag "${RUSTFLAGS:-}" "--remap-path-prefix=$repo_root=.")"
      export RUSTDOCFLAGS="$(sp42_append_flag "${RUSTDOCFLAGS:-}" "--remap-path-prefix=$repo_root=.")"
      ;;
    ci)
      export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
      ;;
    *)
      export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-1}"
      ;;
  esac
}
