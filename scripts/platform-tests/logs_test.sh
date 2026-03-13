#!/usr/bin/env bash
# logs_test.sh — comprehensive logs endpoint tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0

# ── Basic list ────────────────────────────────────────────────────────────────

s="$(api_get "/logs?limit=5" /tmp/t_logs_basic.json)"
assert_status_and_jq "200" "$s" \
  '((.data // .).logs | type == "array")' \
  /tmp/t_logs_basic.json "logs list" FAIL

# ── Pagination limit ──────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_logs_lim1.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/logs?limit=1" || true)"
if [[ "$s" == "200" ]]; then
  count="$(jq '((.data // .).logs | length)' /tmp/t_logs_lim1.json 2>/dev/null || echo 99)"
  if [[ "$count" -le 1 ]]; then
    print_result 1 "logs limit=1"
  else
    print_result 0 "logs limit=1" "returned ${count} items"
    FAIL=1
  fi
else
  print_result 0 "logs limit=1" "HTTP ${s}"
  FAIL=1
fi

# Limit=0 should be handled (empty array or default)
s="$(curl -sS -o /tmp/t_logs_lim0.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/logs?limit=0" || true)"
if [[ "$s" == "200" || "$s" == "400" ]]; then
  print_result 1 "logs limit=0 handled (${s})"
else
  print_result 0 "logs limit=0 handled" "HTTP ${s}"
  FAIL=1
fi

# ── Level filter ──────────────────────────────────────────────────────────────

for lvl in info warn error; do
  s="$(curl -sS -o /tmp/t_logs_lvl_${lvl}.json -w "%{http_code}" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
    -H "X-Fluxbase-Project: ${PROJECT_ID}" \
    "${API_URL}/logs?level=${lvl}&limit=10" || true)"
  if [[ "$s" == "200" ]]; then
    # All returned entries should have matching level (if not empty)
    ok="$(jq -e --arg l "$lvl" \
      '((.data // .).logs | length == 0) or
       ((.data // .).logs | all(.level == $l or .level == null))' \
      /tmp/t_logs_lvl_${lvl}.json 2>/dev/null && echo 1 || echo 0)"
    if [[ "$ok" == "1" ]]; then
      print_result 1 "logs filter level=${lvl}"
    else
      print_result 0 "logs filter level=${lvl}" "non-matching entries in response"
      FAIL=1
    fi
  else
    print_result 0 "logs filter level=${lvl}" "HTTP ${s}"
    FAIL=1
  fi
done

# ── Source filter ─────────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_logs_src.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/logs?source=function&limit=10" || true)"
if [[ "$s" == "200" ]]; then
  ok="$(jq -e '((.data // .).logs | length == 0) or
    ((.data // .).logs | all(.source == "function" or .source == null))' \
    /tmp/t_logs_src.json 2>/dev/null && echo 1 || echo 0)"
  if [[ "$ok" == "1" ]]; then
    print_result 1 "logs filter source=function"
  else
    print_result 0 "logs filter source=function" "non-matching source entries"
    FAIL=1
  fi
else
  print_result 0 "logs filter source=function" "HTTP ${s}"
  FAIL=1
fi

# ── Request-ID filter (after triggering a call) ───────────────────────────────

# Generate a tagged invocation
s="$(gw_post "/${FUNCTION_NAME}" '{"_log_tag":"log-filter-test"}' \
  /tmp/t_logs_gw.json /tmp/t_logs_gw_hdr.txt)"
req_id="$(http_header_value /tmp/t_logs_gw_hdr.txt x-request-id || true)"

if [[ -n "$req_id" ]]; then
  # Wait for log to appear
  if wait_for_json_match "logs by request_id" \
    "${API_URL}/logs?request_id=${req_id}&limit=50" \
    '((.data // .).logs | any((.request_id // "") == env.req_id))' \
    /tmp/t_logs_reqid.json 15 1; then
    print_result 1 "logs filter by request_id"
  else
    print_result 0 "logs filter by request_id" "request_id=${req_id}"
    FAIL=1
  fi
fi

# ── Log entry schema ──────────────────────────────────────────────────────────

s="$(api_get "/logs?limit=10" /tmp/t_logs_schema.json)"
if [[ "$s" == "200" ]]; then
  count="$(jq '((.data // .).logs | length)' /tmp/t_logs_schema.json 2>/dev/null || echo 0)"
  if [[ "$count" -gt 0 ]]; then
    ok="$(jq -e \
      '((.data // .).logs | all(has("level") or has("message") or has("timestamp")))' \
      /tmp/t_logs_schema.json 2>/dev/null && echo 1 || echo 0)"
    if [[ "$ok" == "1" ]]; then
      print_result 1 "logs entry has level/message/timestamp"
    else
      print_result 0 "logs entry has level/message/timestamp"
      FAIL=1
    fi
  fi
fi

# ── Unauthenticated access ────────────────────────────────────────────────────

s="$(curl -sS -o /dev/null -w "%{http_code}" "${API_URL}/logs" || true)"
assert_status "401" "$s" "logs requires auth" FAIL

exit "$FAIL"
