#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

scripts/clean-house.sh

base_url="${SP42_OPERATOR_BASE_URL:-http://127.0.0.1:8788}"
mkdir -p .tmp
server_log="${SP42_OPERATOR_SERVER_LOG:-$repo_root/.tmp/sp42-operator-server.log}"
CARGO_BIN="${CARGO_BIN:-$(command -v cargo)}"
WORKSPACE_TARGET_DIR="${CARGO_TARGET_DIR_WORKSPACE:-$repo_root/target/workspace}"
TAURI_TARGET_DIR="${CARGO_TARGET_DIR_TAURI:-$repo_root/target/tauri}"
mkdir -p "$WORKSPACE_TARGET_DIR" "$TAURI_TARGET_DIR"

cleanup() {
  if [[ -n "${server_pid:-}" ]]; then
    kill "$server_pid" >/dev/null 2>&1 || true
    wait "$server_pid" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

run_step() {
  printf '\n== %s ==\n' "$1"
  shift
  "$@"
}

fetch_json() {
  local url="$1"
  local target="$2"
  /usr/bin/curl -fsS "$url" -o "$target"
}

assert_json_field() {
  local file="$1"
  local path="$2"
  local expected="$3"
  python3 - "$file" "$path" "$expected" <<'PY'
import json
import sys
from pathlib import Path

file_path, path, expected_raw = sys.argv[1:]
data = json.loads(Path(file_path).read_text())
value = data
for part in path.split('.'):
    if not part:
        continue
    if isinstance(value, list):
        value = value[int(part)]
    else:
        value = value[part]

try:
    expected = json.loads(expected_raw)
except json.JSONDecodeError:
    expected = expected_raw

if value != expected:
    raise SystemExit(f"{path} mismatch: expected {expected!r}, got {value!r}")
PY
}

assert_json_list_contains() {
  local file="$1"
  local path="$2"
  local needle="$3"
  python3 - "$file" "$path" "$needle" <<'PY'
import json
import sys
from pathlib import Path

file_path, path, needle = sys.argv[1:]
data = json.loads(Path(file_path).read_text())
value = data
for part in path.split('.'):
    if not part:
        continue
    if isinstance(value, list):
        value = value[int(part)]
    else:
        value = value[part]

if not isinstance(value, list):
    raise SystemExit(f"{path} is not a list")

if not any(
    item == needle or (isinstance(item, dict) and item.get('path') == needle)
    for item in value
):
    raise SystemExit(f"{path} does not contain {needle!r}")
PY
}

wait_for_server() {
  for _ in $(seq 1 120); do
    if /usr/bin/curl -fsS "${base_url}/healthz" >/dev/null 2>&1; then
      return 0
    fi
    sleep 1
  done

  echo "Local server did not become ready on ${base_url}" >&2
  if [[ -f "$server_log" ]]; then
    tail -n 80 "$server_log" >&2 || true
  fi
  return 1
}

run_step "core recentchanges tests" "$CARGO_BIN" test -p sp42-core recent_changes
run_step "core backlog tests" "$CARGO_BIN" test -p sp42-core backlog_runtime
run_step "server multi-user coordination test" "$CARGO_BIN" test -p sp42-server multi_user_coordination_flow_round_trips_across_authenticated_clients
run_step "shell builds" "$CARGO_BIN" build -p sp42-server -p sp42-cli -p sp42-desktop
run_step "browser wasm build" "$CARGO_BIN" build -p sp42-app --target wasm32-unknown-unknown

stale_server_pids="$(lsof -tiTCP:8788 -sTCP:LISTEN 2>/dev/null || true)"
if [[ -n "$stale_server_pids" ]]; then
  for stale_pid in $stale_server_pids; do
    if ps -p "$stale_pid" -o comm= | grep -q 'sp42-server'; then
      printf '\n== stop stale local server ==\n'
      kill "$stale_pid" >/dev/null 2>&1 || true
      wait "$stale_pid" >/dev/null 2>&1 || true
    else
      echo "Port 8788 is occupied by non-SP42 process ${stale_pid}; refusing to continue." >&2
      exit 1
    fi
  done
fi

printf '\n== start local server ==\n'
"$CARGO_BIN" run -q -p sp42-server >"$server_log" 2>&1 &
server_pid="$!"
wait_for_server

healthz_json="$(mktemp .tmp/sp42-healthz.XXXXXX)"
summary_json="$(mktemp .tmp/sp42-summary.XXXXXX)"
runtime_json="$(mktemp .tmp/sp42-runtime.XXXXXX)"
bootstrap_json="$(mktemp .tmp/sp42-bootstrap.XXXXXX)"
session_json="$(mktemp .tmp/sp42-session.XXXXXX)"
status_json="$(mktemp .tmp/sp42-status.XXXXXX)"
history_json="$(mktemp .tmp/sp42-history.XXXXXX)"
inspections_json="$(mktemp .tmp/sp42-inspections.XXXXXX)"
room_json="$(mktemp .tmp/sp42-room.XXXXXX)"
room_inspection_json="$(mktemp .tmp/sp42-room-inspection.XXXXXX)"
readiness_json="$(mktemp .tmp/sp42-readiness.XXXXXX)"
report_json="$(mktemp .tmp/sp42-report.XXXXXX)"

run_step "server health probe" fetch_json "${base_url}/healthz" "$healthz_json"
run_step "server debug summary" fetch_json "${base_url}/debug/summary" "$summary_json"
run_step "server runtime debug" fetch_json "${base_url}/debug/runtime" "$runtime_json"
run_step "bootstrap status surface" fetch_json "${base_url}/dev/auth/bootstrap/status" "$bootstrap_json"
run_step "action status surface" fetch_json "${base_url}/dev/actions/status" "$status_json"
run_step "action history surface" fetch_json "${base_url}/dev/actions/history?limit=1" "$history_json"
run_step "coordination snapshot surface" fetch_json "${base_url}/coordination/rooms" "$room_json"
run_step "coordination inspections surface" fetch_json "${base_url}/coordination/inspections" "$inspections_json"
run_step "coordination room inspection surface" fetch_json "${base_url}/coordination/rooms/frwiki/inspection" "$room_inspection_json"
run_step "operator readiness surface" fetch_json "${base_url}/operator/readiness" "$readiness_json"
run_step "operator report surface" fetch_json "${base_url}/operator/report" "$report_json"

local_ready="$(python3 - "$bootstrap_json" <<'PY'
import json
import sys
from pathlib import Path

data = json.loads(Path(sys.argv[1]).read_text())
print("true" if data.get("bootstrap_ready") else "false")
PY
)"

assert_json_field "$healthz_json" project '"SP42"'
assert_json_field "$healthz_json" ready_for_local_testing "$local_ready"
assert_json_field "$healthz_json" coordination_room_count 0
assert_json_field "$summary_json" project '"SP42"'
assert_json_field "$summary_json" coordination.rooms '[]'
assert_json_field "$runtime_json" project '"SP42"'
assert_json_field "$runtime_json" coordination_room_count 0
assert_json_field "$runtime_json" coordination.rooms '[]'
assert_json_field "$runtime_json" bootstrap.source_report.file_name '".env.wikimedia.local"'
assert_json_field "$bootstrap_json" source_report.file_name '".env.wikimedia.local"'
assert_json_field "$bootstrap_json" bootstrap_ready "$local_ready"
assert_json_field "$bootstrap_json" oauth.access_token_present "$local_ready"
assert_json_field "$bootstrap_json" session.authenticated false
assert_json_field "$status_json" authenticated false
assert_json_field "$status_json" total_actions 0
assert_json_field "$status_json" last_execution null
assert_json_field "$history_json" authenticated false
assert_json_field "$history_json" entries '[]'
assert_json_field "$room_json" rooms '[]'
assert_json_field "$inspections_json" rooms '[]'
assert_json_field "$room_inspection_json" room.wiki_id '"frwiki"'
assert_json_field "$room_inspection_json" metrics.accepted_messages 0
assert_json_field "$readiness_json" operator_report_path '"/operator/report"'
assert_json_field "$readiness_json" coordination_room_count 0
assert_json_field "$report_json" project '"SP42"'
assert_json_list_contains "$report_json" endpoints '/dev/actions/history'
assert_json_list_contains "$report_json" endpoints '/operator/live/{wiki_id}'

if [[ "$local_ready" == true ]]; then
  cookie_jar="$(mktemp .tmp/sp42-cookie.XXXXXX)"
  /usr/bin/curl -fsS \
    -c "$cookie_jar" \
    -H 'content-type: application/json' \
    -X POST \
    --data '{}' \
    "${base_url}/dev/auth/session/bootstrap" \
    -o "$bootstrap_json"

  assert_json_field "$bootstrap_json" authenticated true
  assert_json_field "$bootstrap_json" bridge_mode '"local-env-token"'
  assert_json_field "$bootstrap_json" token_present true

  /usr/bin/curl -fsS -b "$cookie_jar" "${base_url}/dev/auth/session" -o "$session_json"
  /usr/bin/curl -fsS -b "$cookie_jar" "${base_url}/dev/actions/status" -o "$status_json"
  /usr/bin/curl -fsS -b "$cookie_jar" "${base_url}/dev/actions/history?limit=1" -o "$history_json"

  assert_json_field "$session_json" authenticated true
  assert_json_field "$session_json" bridge_mode '"local-env-token"'
  assert_json_field "$session_json" token_present true
  assert_json_field "$status_json" authenticated true
  assert_json_field "$history_json" authenticated true
  assert_json_field "$history_json" entries '[]'
fi

run_step "server health probe" /usr/bin/curl -fsS "${base_url}/healthz"
run_step "server debug summary" /usr/bin/curl -fsS "${base_url}/debug/summary"
run_step "server runtime debug" /usr/bin/curl -fsS "${base_url}/debug/runtime"
run_step "action history surface" /usr/bin/curl -fsS "${base_url}/dev/actions/history?limit=1"
run_step "coordination inspections" /usr/bin/curl -fsS "${base_url}/coordination/inspections"
run_step "cli parity report" "$CARGO_BIN" run -q -p sp42-cli -- --shell parity-report --format markdown
run_step "cli session digest" "$CARGO_BIN" run -q -p sp42-cli -- --shell session-digest --workbench-token local-smoke-token --workbench-actor smoke-tester --format text
run_step "desktop shell snapshot" "$CARGO_BIN" run -q -p sp42-desktop -- --format json
run_step "tauri shell contract" env CARGO_TARGET_DIR="$TAURI_TARGET_DIR" "$CARGO_BIN" run -q --manifest-path crates/sp42-desktop/src-tauri/Cargo.toml

printf '\nSP42 local operator smoke flow passed.\n'
