#!/usr/bin/env bash
# run_all.sh — orchestrate all platform test scripts
#
# Usage:
#   ./run_all.sh [--fail-fast] [--skip <script1,script2,...>] [--only <script1,...>]
#
# Exit: 0 = all passed, 1 = some failed
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

require_env DB_URL RUNTIME_URL QUEUE_URL

# ── Argument parsing ──────────────────────────────────────────────────────────

FAIL_FAST=0
SKIP_LIST=""
ONLY_LIST=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --fail-fast) FAIL_FAST=1 ;;
    --skip) shift; SKIP_LIST="$1" ;;
    --only) shift; ONLY_LIST="$1" ;;
    *) echo "Unknown option: $1" >&2; exit 2 ;;
  esac
  shift
done

should_run() {
  local name="$1"
  if [[ -n "$ONLY_LIST" ]]; then
    printf '%s' "$ONLY_LIST" | tr ',' '\n' | grep -qx "$name" && return 0 || return 1
  fi
  if [[ -n "$SKIP_LIST" ]]; then
    printf '%s' "$SKIP_LIST" | tr ',' '\n' | grep -qx "$name" && return 1 || return 0
  fi
  return 0
}

# ── Counters and timing ───────────────────────────────────────────────────────

TOTAL=0
PASSED=0
FAILED=0
START_ALL="$(date +%s)"
SUITE_FAIL=0

record_line() {
  local line="$1"
  echo "$line"
  if [[ "$line" =~ ^\[PASS\] ]]; then
    PASSED=$((PASSED + 1)); TOTAL=$((TOTAL + 1))
  elif [[ "$line" =~ ^\[FAIL\] ]]; then
    FAILED=$((FAILED + 1)); TOTAL=$((TOTAL + 1))
    SUITE_FAIL=1
  fi
}

# ── Health pre-flight ─────────────────────────────────────────────────────────

health_check_one() {
  local url="$1" label="$2"
  local s body
  s="$(curl -sS -o /tmp/t_rall_hc.json -w "%{http_code}" "${url}/health" 2>/dev/null || true)"
  body="$(cat /tmp/t_rall_hc.json 2>/dev/null || echo '{}')"
  if [[ "$s" == "200" ]] && printf '%s' "$body" | jq -e '.status == "ok"' >/dev/null 2>&1; then
    record_line "[PASS] health:${label}"
    return 0
  else
    record_line "[FAIL] health:${label} (HTTP ${s})"
    return 1
  fi
}

echo "────────────────────────────────────────"
echo "  Flux Platform Test Suite"
echo "────────────────────────────────────────"
echo "  API      : ${API_URL}"
echo "  Gateway  : ${GATEWAY_URL}"
echo "  DB       : ${DB_URL}"
echo "  Runtime  : ${RUNTIME_URL}"
echo "  Queue    : ${QUEUE_URL}"
echo "────────────────────────────────────────"

hc_ok=1
for svc_entry in "API_URL:api" "GATEWAY_URL:gateway" "DB_URL:db" "RUNTIME_URL:runtime" "QUEUE_URL:queue"; do
  var="${svc_entry%%:*}"; label="${svc_entry##*:}"
  url="${!var}"
  health_check_one "$url" "$label" || hc_ok=0
done

if [[ "$hc_ok" -eq 0 ]]; then
  echo
  echo "[ERROR] One or more services are not healthy. Aborting."
  exit 1
fi

echo

# ── Script runner ─────────────────────────────────────────────────────────────

run_script() {
  local script="$1"
  local label="${script%.sh}"

  if ! should_run "$script"; then
    echo "  [SKIP] ${label}"
    return
  fi

  local t0 t1 elapsed output exit_code
  t0="$(date +%s)"
  output="$("$DIR/$script" 2>&1)" || exit_code=$?
  exit_code="${exit_code:-0}"
  t1="$(date +%s)"
  elapsed="$((t1 - t0))s"

  local pass_count fail_count
  pass_count="$(printf '%s\n' "$output" | grep -c '^\[PASS\]' || true)"
  fail_count="$(printf '%s\n' "$output" | grep -c '^\[FAIL\]' || true)"

  printf "  %-40s  %3d PASS  %3d FAIL  (%s)\n" \
    "${label}" "$pass_count" "$fail_count" "$elapsed"

  while IFS= read -r line; do
    [[ -n "$line" ]] && record_line "$line"
  done <<< "$output"

  if [[ "$FAIL_FAST" -eq 1 && "$SUITE_FAIL" -eq 1 ]]; then
    echo
    echo "[FAIL-FAST] Stopping after first failure in ${label}."
    summary
    exit 1
  fi
}

# ── Test execution order ──────────────────────────────────────────────────────
# Core → auth → functions → gateway → runtime → services → observability → load

echo "Running test scripts..."
echo

run_script "schema_test.sh"
run_script "auth_test.sh"
run_script "api_test.sh"
run_script "server_test.sh"
run_script "db_test.sh"
run_script "data_engine_test.sh"
run_script "file_test.sh"
run_script "function_test.sh"
run_script "gateway_test.sh"
run_script "runtime_test.sh"
run_script "queue_service_test.sh"
run_script "agent_test.sh"
run_script "cli_test.sh"
run_script "execution_record_test.sh"
run_script "state_audit_test.sh"
run_script "logs_test.sh"
run_script "events_test.sh"
run_script "load_test.sh"

# ── Summary ───────────────────────────────────────────────────────────────────

END_ALL="$(date +%s)"
TOTAL_TIME="$((END_ALL - START_ALL))"

echo
echo "════════════════════════════════════════"
printf "  Total:   %d\n" "$TOTAL"
printf "  Passed:  %d\n" "$PASSED"
printf "  Failed:  %d\n" "$FAILED"
printf "  Time:    %ds\n" "$TOTAL_TIME"
echo "════════════════════════════════════════"

if [[ "$FAILED" -eq 0 ]]; then
  echo "  ✓ All tests passed"
  exit 0
else
  echo "  ✗ ${FAILED} test(s) failed"
  exit 1
fi
