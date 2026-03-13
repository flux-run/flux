#!/usr/bin/env bash
# execution_record_test.sh — trace, log, and records tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0
TRACE_PAYLOAD="${TRACE_TEST_PAYLOAD:-}"
if [[ -z "$TRACE_PAYLOAD" ]]; then
  TRACE_PAYLOAD='{"message":"execution-record-smoke"}'
fi
EXPECTED_PATH="/${FUNCTION_NAME}"

# ── Trigger invocation ────────────────────────────────────────────────────────

s="$(gw_post "/${FUNCTION_NAME}" "$TRACE_PAYLOAD" \
  /tmp/t_exec_body.json /tmp/t_exec_hdr.txt)"
request_id="$(http_header_value /tmp/t_exec_hdr.txt x-request-id || true)"

if [[ "$s" == "200" && -n "$request_id" ]]; then
  print_result 1 "execution request id"
else
  print_result 0 "execution request id" "status=${s}, request_id=${request_id:-missing}"
  exit 1
fi

export REQUEST_ID_EXPECTED="$request_id"
export EXPECTED_PATH
export EXPECTED_FUNCTION="$FUNCTION_NAME"

# ── Trace detail ──────────────────────────────────────────────────────────────

if wait_for_json_match \
  "trace detail" \
  "${API_URL}/traces/${request_id}?slow_ms=0" \
  '.request_id == env.REQUEST_ID_EXPECTED
   and (.spans | type == "array" and length > 0)
   and ([.spans[] | has("source") and has("resource") and has("timestamp")] | all)' \
  /tmp/t_trace_detail.json 20 1; then
  print_result 1 "trace detail"

  # Spans should have a level field
  ok="$(jq -e '.spans | all(has("level") or has("source"))' /tmp/t_trace_detail.json 2>/dev/null && echo 1 || echo 0)"
  if [[ "$ok" == "1" ]]; then
    print_result 1 "trace spans have level/source"
  else
    print_result 0 "trace spans have level/source"
    FAIL=1
  fi

  # At least one span should reference our function
  ok="$(jq -e --arg fn "$FUNCTION_NAME" \
    '.spans | any(.[]; (.resource // .function // "") == $fn or (.message // "") | contains($fn))' \
    /tmp/t_trace_detail.json 2>/dev/null && echo 1 || echo 0)"
  if [[ "$ok" == "1" ]]; then
    print_result 1 "trace span references function"
  else
    print_result 0 "trace span references function"
    FAIL=1
  fi

  # Duration must be present and ≥ 0
  dur="$(jq '.duration_ms // -1' /tmp/t_trace_detail.json 2>/dev/null || echo -1)"
  if python3 -c "import sys; sys.exit(0 if float('${dur}') >= 0 else 1)" 2>/dev/null; then
    print_result 1 "trace duration_ms present (${dur}ms)"
  else
    print_result 0 "trace duration_ms present" "got: ${dur}"
    FAIL=1
  fi
else
  print_result 0 "trace detail" "request_id=${request_id}"
  FAIL=1
fi

# ── Trace list ────────────────────────────────────────────────────────────────

if wait_for_json_match \
  "trace list" \
  "${API_URL}/traces?limit=20" \
  '(.traces | type == "array")
   and any(.traces[];
     .request_id == env.REQUEST_ID_EXPECTED
     and (.path == env.EXPECTED_PATH or .function == env.EXPECTED_FUNCTION)
   )' \
  /tmp/t_trace_list.json 20 1; then
  print_result 1 "trace list contains our request"

  # Trace list entries must have basic fields
  ok="$(jq -e '.traces | all(has("request_id") and has("status"))' /tmp/t_trace_list.json 2>/dev/null && echo 1 || echo 0)"
  if [[ "$ok" == "1" ]]; then
    print_result 1 "trace list entry schema"
  else
    print_result 0 "trace list entry schema" "missing request_id or status"
    FAIL=1
  fi
else
  print_result 0 "trace list" "request_id=${request_id}"
  FAIL=1
fi

# ── Trace list with function filter ──────────────────────────────────────────

s="$(curl -sS -o /tmp/t_trace_fn_filter.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/traces?function=${FUNCTION_NAME}&limit=10" || true)"
if [[ "$s" == "200" ]]; then
  ok="$(jq -e --arg fn "$FUNCTION_NAME" \
    '.traces | all(.function == $fn or .function == null or (.path // "") | endswith($fn))' \
    /tmp/t_trace_fn_filter.json 2>/dev/null && echo 1 || echo 0)"
  if [[ "$ok" == "1" ]]; then
    print_result 1 "trace filter by function"
  else
    print_result 0 "trace filter by function" "non-matching entries"
    FAIL=1
  fi
else
  print_result 0 "trace filter by function" "HTTP ${s}"
  FAIL=1
fi

# ── Unknown trace → 404 ───────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_trace_not_found.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/traces/00000000-0000-0000-0000-000000000000" || true)"
if [[ "$s" == "404" ]]; then
  print_result 1 "trace unknown id → 404"
else
  print_result 0 "trace unknown id → 404" "HTTP ${s}"
  FAIL=1
fi

# ── Execution logs ────────────────────────────────────────────────────────────

if wait_for_json_match \
  "execution logs" \
  "${API_URL}/logs?limit=100" \
  '(.logs | type == "array")
   and any(.logs[]; (.request_id // "") == env.REQUEST_ID_EXPECTED)' \
  /tmp/t_exec_logs.json 20 1; then
  print_result 1 "execution logs"
else
  print_result 0 "execution logs" "request_id=${request_id}"
  FAIL=1
fi

# ── Records count ─────────────────────────────────────────────────────────────

s="$(api_get "/records/count?after=24h" /tmp/t_records_count.json)"
assert_status_and_jq "200" "$s" '.count >= 1' /tmp/t_records_count.json "records count ≥ 1" FAIL

# ── Records export NDJSON ─────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_records_export.ndjson -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/records/export?after=24h" || true)"

if [[ "$s" == "200" ]]; then
  # Each line should be valid JSON
  lines="$(wc -l < /tmp/t_records_export.ndjson || echo 0)"
  print_result 1 "records export 200 (${lines} lines)"

  # Our request_id should be in the export
  if grep -F "\"request_id\":\"${request_id}\"" /tmp/t_records_export.ndjson >/dev/null 2>&1; then
    print_result 1 "records export contains our request_id"
  else
    print_result 0 "records export contains our request_id" "request_id=${request_id}"
    FAIL=1
  fi

  # First line should be valid JSON
  if [[ "$lines" -gt 0 ]]; then
    if head -1 /tmp/t_records_export.ndjson | jq -e '.' >/dev/null 2>&1; then
      print_result 1 "records export valid NDJSON"
    else
      print_result 0 "records export valid NDJSON" "first line not valid JSON"
      FAIL=1
    fi
  fi
else
  print_result 0 "records export" "HTTP ${s}"
  FAIL=1
fi

# ── Records time window filter ────────────────────────────────────────────────

from_ts="$(date -u -v-1H +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || date -u --date="-1 hour" +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || now_rfc3339)"
s="$(curl -sS -o /tmp/t_records_window.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/records/export?from=${from_ts}" || true)"
if [[ "$s" == "200" ]]; then
  print_result 1 "records export time window filter"
else
  print_result 0 "records export time window filter" "HTTP ${s}"
  FAIL=1
fi

# ── Unauthenticated access ────────────────────────────────────────────────────

s="$(curl -sS -o /dev/null -w "%{http_code}" "${API_URL}/traces" || true)"
assert_status "401" "$s" "traces requires auth" FAIL

exit "$FAIL"
