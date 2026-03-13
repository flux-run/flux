#!/usr/bin/env bash
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

status="$(curl -sS -D /tmp/platform_exec_headers.txt -o /tmp/platform_exec_body.json -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/${FUNCTION_NAME}" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data "${TRACE_PAYLOAD}")"

request_id="$(http_header_value /tmp/platform_exec_headers.txt x-request-id || true)"

if [[ "$status" == "200" && -n "$request_id" ]]; then
  print_result 1 "execution request id"
else
  print_result 0 "execution request id" "status=${status}, request_id=${request_id:-missing}"
  exit 1
fi

export REQUEST_ID_EXPECTED="$request_id"
export EXPECTED_PATH
export EXPECTED_FUNCTION="$FUNCTION_NAME"

if wait_for_json_match \
  "trace detail" \
  "${API_URL}/traces/${request_id}?slow_ms=0" \
  '.request_id == env.REQUEST_ID_EXPECTED
   and (.spans | type == "array" and length > 0)
   and ([.spans[] | has("source") and has("resource") and has("timestamp")] | all)' \
  /tmp/platform_trace_detail.json 20 1; then
  print_result 1 "trace detail"
else
  print_result 0 "trace detail" "request_id=${request_id}"
  FAIL=1
fi

if wait_for_json_match \
  "trace list" \
  "${API_URL}/traces?limit=20" \
  '(.traces | type == "array")
   and any(.traces[];
     .request_id == env.REQUEST_ID_EXPECTED
     and (.path == env.EXPECTED_PATH or .function == env.EXPECTED_FUNCTION)
   )' \
  /tmp/platform_trace_list.json 20 1; then
  print_result 1 "trace list"
else
  print_result 0 "trace list" "request_id=${request_id}"
  FAIL=1
fi

if wait_for_json_match \
  "project logs" \
  "${API_URL}/logs?limit=100" \
  '(.logs | type == "array")
   and any(.logs[]; (.request_id // "") == env.REQUEST_ID_EXPECTED)' \
  /tmp/platform_logs_for_request.json 20 1; then
  print_result 1 "execution logs"
else
  print_result 0 "execution logs" "request_id=${request_id}"
  FAIL=1
fi

records_count_status="$(curl -sS -o /tmp/platform_records_count.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/records/count?after=24h")"

if [[ "$records_count_status" == "200" ]] && jq -e '.count >= 1' /tmp/platform_records_count.json >/dev/null; then
  print_result 1 "records count"
else
  print_result 0 "records count" "status=${records_count_status}"
  FAIL=1
fi

records_export_status="$(curl -sS -o /tmp/platform_records_export.ndjson -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/records/export?after=24h")"

if [[ "$records_export_status" == "200" ]] \
  && grep -F "\"request_id\":\"${request_id}\"" /tmp/platform_records_export.ndjson >/dev/null 2>&1; then
  print_result 1 "records export"
else
  print_result 0 "records export" "status=${records_export_status}, request_id=${request_id}"
  FAIL=1
fi

exit "$FAIL"
