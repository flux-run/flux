#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

require_env DB_URL RUNTIME_URL QUEUE_URL

TOTAL=0
PASSED=0
FAILED=0

record_line() {
  local line="$1"
  echo "$line"
  if [[ "$line" =~ ^\[PASS\] ]]; then
    PASSED=$((PASSED + 1))
    TOTAL=$((TOTAL + 1))
  elif [[ "$line" =~ ^\[FAIL\] ]]; then
    FAILED=$((FAILED + 1))
    TOTAL=$((TOTAL + 1))
  fi
}

health_check_one() {
  local url="$1"
  local status body
  status="$(curl -sS -o /tmp/platform_health_body.json -w "%{http_code}" "${url}/health" || true)"
  body="$(cat /tmp/platform_health_body.json 2>/dev/null || true)"
  [[ "$status" == "200" ]] && echo "$body" | jq -e '.status == "ok"' >/dev/null 2>&1
}

if health_check_one "$API_URL" \
  && health_check_one "$GATEWAY_URL" \
  && health_check_one "$DB_URL" \
  && health_check_one "$RUNTIME_URL" \
  && health_check_one "$QUEUE_URL"; then
  record_line "[PASS] health"
else
  record_line "[FAIL] health"
fi

run_script() {
  local script="$1"
  local output
  output="$("$DIR/$script" 2>&1 || true)"
  while IFS= read -r line; do
    [[ -n "$line" ]] && record_line "$line"
  done <<< "$output"
}

run_script "schema_test.sh"
run_script "db_test.sh"
run_script "file_test.sh"
run_script "function_test.sh"
run_script "logs_test.sh"
run_script "events_test.sh"
run_script "load_test.sh"
run_script "auth_test.sh"

echo
printf 'Total: %d\nPassed: %d\nFailed: %d\n' "$TOTAL" "$PASSED" "$FAILED"

if [[ "$FAILED" -eq 0 ]]; then
  exit 0
fi
exit 1
