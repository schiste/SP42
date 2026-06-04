#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

source "$repo_root/scripts/lib/build-common.sh"
sp42_setup_build_env "$repo_root" dev
export SP42_APP_DIST_DIR="${SP42_APP_DIST_DIR:-$(sp42_frontend_dist_dir "$repo_root")}"

server_bind="127.0.0.1:8788"
trunk_address="127.0.0.1"
trunk_port="4173"
run_smoke=0

usage() {
  cat <<'EOF'
Usage: ./scripts/dev-local.sh [--smoke]

Starts the SP42 local development stack:
  - sp42-server on 127.0.0.1:8788
  - trunk serve on 127.0.0.1:4173

Options:
  --smoke   Run lightweight HTTP smoke checks after both servers are ready.
EOF
}

for arg in "$@"; do
  case "$arg" in
    --smoke)
      run_smoke=1
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "Unsupported option: $arg" >&2
      usage >&2
      exit 1
      ;;
  esac
done

CARGO_BIN="$(sp42_cargo_bin)"
TRUNK_BIN="${TRUNK_BIN:-trunk}"
if ! command -v "$TRUNK_BIN" >/dev/null 2>&1; then
  echo "trunk is required for ./scripts/dev-local.sh" >&2
  exit 1
fi

mkdir -p .tmp
mkdir -p "$SP42_APP_DIST_DIR"
server_log="${SP42_DEV_SERVER_LOG:-$repo_root/.tmp/sp42-dev-server.log}"
trunk_log="${SP42_DEV_TRUNK_LOG:-$repo_root/.tmp/sp42-dev-trunk.log}"
: >"$server_log"
: >"$trunk_log"

cleanup() {
  if [[ -n "${trunk_pid:-}" ]]; then
    kill "$trunk_pid" >/dev/null 2>&1 || true
    wait "$trunk_pid" >/dev/null 2>&1 || true
  fi
  if [[ -n "${server_pid:-}" ]]; then
    kill "$server_pid" >/dev/null 2>&1 || true
    wait "$server_pid" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT TERM

wait_for_url() {
  local label="$1"
  local url="$2"
  local pid="$3"
  local log_file="$4"

  for _ in $(seq 1 120); do
    if /usr/bin/curl -fsS "$url" >/dev/null 2>&1; then
      printf '%s ready at %s\n' "$label" "$url"
      return 0
    fi
    if ! kill -0 "$pid" >/dev/null 2>&1; then
      echo "$label exited before becoming ready. Recent log:" >&2
      tail -n 80 "$log_file" >&2 || true
      return 1
    fi
    sleep 1
  done

  echo "$label did not become ready at $url. Recent log:" >&2
  tail -n 80 "$log_file" >&2 || true
  return 1
}

run_smoke_checks() {
  local server_url="http://${server_bind}"
  local trunk_url="http://${trunk_address}:${trunk_port}"

  /usr/bin/curl -fsS "${server_url}/healthz" >/dev/null
  /usr/bin/curl -fsS "${server_url}/debug/runtime" >/dev/null
  /usr/bin/curl -fsS "${server_url}/dev/auth/bootstrap/status" >/dev/null
  /usr/bin/curl -fsS "${trunk_url}/" >/dev/null
  printf 'Local dev smoke checks passed.\n'
}

printf 'Starting sp42-server on %s\n' "$server_bind"
SP42_BIND_ADDR="$server_bind" "$CARGO_BIN" run -q -p sp42-server >"$server_log" 2>&1 &
server_pid="$!"

printf 'Starting Trunk on %s:%s\n' "$trunk_address" "$trunk_port"
"$TRUNK_BIN" serve \
  --config "$repo_root/Trunk.toml" \
  --address "$trunk_address" \
  --port "$trunk_port" \
  >"$trunk_log" 2>&1 &
trunk_pid="$!"

wait_for_url "sp42-server" "http://${server_bind}/healthz" "$server_pid" "$server_log"
wait_for_url "Trunk" "http://${trunk_address}:${trunk_port}/" "$trunk_pid" "$trunk_log"

if [[ "$run_smoke" == "1" ]]; then
  run_smoke_checks
fi

cat <<EOF

SP42 local development stack is running.
  App:    http://${trunk_address}:${trunk_port}
  Server: http://${server_bind}
  Logs:   $server_log
          $trunk_log

Press Ctrl-C to stop both processes.
EOF

while true; do
  if ! kill -0 "$server_pid" >/dev/null 2>&1; then
    echo "sp42-server exited. Recent log:" >&2
    tail -n 80 "$server_log" >&2 || true
    exit 1
  fi
  if ! kill -0 "$trunk_pid" >/dev/null 2>&1; then
    echo "Trunk exited. Recent log:" >&2
    tail -n 80 "$trunk_log" >&2 || true
    exit 1
  fi
  sleep 1
done
