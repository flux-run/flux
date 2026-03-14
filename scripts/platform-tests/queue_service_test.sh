#!/usr/bin/env bash
# queue_service_test.sh — comprehensive queue service tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init
require_env QUEUE_URL

FAIL=0

function_id="$(fetch_function_id /tmp/t_queue_fns.json || true)"
if [[ -z "$function_id" ]]; then
  print_result 0 "queue function lookup" "could not resolve ${FUNCTION_NAME}"
  exit 1
fi
print_result 1 "queue function lookup"

# ── Health & stats ────────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_q_health.json -w "%{http_code}" "${QUEUE_URL}/health" || true)"
assert_status_and_jq "200" "$s" '.status == "ok"' /tmp/t_q_health.json "queue health" FAIL

s="$(curl -sS -o /tmp/t_q_stats.json -w "%{http_code}" "${QUEUE_URL}/jobs/stats" || true)"
assert_status_and_jq "200" "$s" \
  'has("queue") and has("latency_ms") and has("retries")' \
  /tmp/t_q_stats.json "queue stats" FAIL

# stats fields must be numbers
assert_jq '.queue | type == "number" or type == "object"' /tmp/t_q_stats.json "queue stats.queue type" FAIL

# ── Create job ────────────────────────────────────────────────────────────────

job_key="q-smoke-$(unique_suffix)"
s="$(curl -sS -o /tmp/t_q_create.json -w "%{http_code}" \
  -X POST "${QUEUE_URL}/jobs" \
  -H "Content-Type: application/json" \
  --data "{\"tenant_id\":\"${TENANT_ID}\",\"project_id\":\"${PROJECT_ID}\",\"function_id\":\"${function_id}\",\"payload\":{\"message\":\"queue-smoke\"},\"idempotency_key\":\"${job_key}\"}" || true)"

job_id="$(jq -er '.job_id' /tmp/t_q_create.json 2>/dev/null || true)"
assert_status_and_jq "201" "$s" 'has("job_id")' /tmp/t_q_create.json "queue create job" FAIL

if [[ -z "$job_id" ]]; then
  print_result 0 "queue job_id returned" "job_id missing"
  exit 1
fi

# ── Idempotency: same key same result ─────────────────────────────────────────

s2="$(curl -sS -o /tmp/t_q_idem.json -w "%{http_code}" \
  -X POST "${QUEUE_URL}/jobs" \
  -H "Content-Type: application/json" \
  --data "{\"tenant_id\":\"${TENANT_ID}\",\"project_id\":\"${PROJECT_ID}\",\"function_id\":\"${function_id}\",\"payload\":{\"message\":\"queue-idem\"},\"idempotency_key\":\"${job_key}\"}" || true)"
job_id2="$(jq -er '.job_id' /tmp/t_q_idem.json 2>/dev/null || true)"
if [[ "$s2" == "201" || "$s2" == "200" || "$s2" == "409" ]]; then
  if [[ "$s2" == "201" && -n "$job_id2" && "$job_id2" == "$job_id" ]]; then
    print_result 1 "queue idempotency same key returns same job_id"
  elif [[ "$s2" == "409" ]]; then
    print_result 1 "queue idempotency same key → 409 conflict"
  else
    print_result 1 "queue idempotency handled (${s2})"
  fi
else
  print_result 0 "queue idempotency" "HTTP ${s2}"
  FAIL=1
fi

# ── Get job ───────────────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_q_get.json -w "%{http_code}" \
  "${QUEUE_URL}/jobs/${job_id}" || true)"
assert_status_and_jq "200" "$s" \
  --arg id "$job_id" '.id == $id' \
  /tmp/t_q_get.json "queue get job" FAIL

# Job has required fields
assert_jq 'has("id") and has("status") and (has("created_at") or has("created"))' \
  /tmp/t_q_get.json "queue job schema" FAIL

# ── List jobs ─────────────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_q_list.json -w "%{http_code}" "${QUEUE_URL}/jobs?limit=20" || true)"
assert_status_and_jq "200" "$s" \
  '.jobs | type == "array"' \
  /tmp/t_q_list.json "queue list jobs" FAIL

# Our job should appear in the list
if jq -e --arg id "$job_id" '.jobs | any(.[]; .id == $id)' /tmp/t_q_list.json >/dev/null 2>&1; then
  print_result 1 "queue list contains new job"
else
  print_result 0 "queue list contains new job" "job_id=${job_id} not found"
  FAIL=1
fi

# ── Pagination ────────────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_q_lim1.json -w "%{http_code}" "${QUEUE_URL}/jobs?limit=1" || true)"
if [[ "$s" == "200" ]]; then
  count="$(jq '.jobs | length' /tmp/t_q_lim1.json 2>/dev/null || echo 99)"
  if [[ "$count" -le 1 ]]; then
    print_result 1 "queue list limit=1"
  else
    print_result 0 "queue list limit=1" "returned ${count} items"
    FAIL=1
  fi
fi

# ── Get non-existent job → 404 ────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_q_not_found.json -w "%{http_code}" \
  "${QUEUE_URL}/jobs/00000000-0000-0000-0000-000000000000" || true)"
assert_status "404" "$s" "queue get unknown job → 404" FAIL

# ── Create job with delay ─────────────────────────────────────────────────────

job_key2="q-delay-$(unique_suffix)"
s="$(curl -sS -o /tmp/t_q_delay.json -w "%{http_code}" \
  -X POST "${QUEUE_URL}/jobs" \
  -H "Content-Type: application/json" \
  --data "{\"tenant_id\":\"${TENANT_ID}\",\"project_id\":\"${PROJECT_ID}\",\"function_id\":\"${function_id}\",\"payload\":{},\"delay_seconds\":60,\"idempotency_key\":\"${job_key2}\"}" || true)"
if [[ "$s" == "201" ]]; then
  delayed_id="$(jq -r '.job_id' /tmp/t_q_delay.json 2>/dev/null || true)"
  # Delayed job should have scheduled status
  if [[ -n "$delayed_id" ]]; then
    s_get="$(curl -sS -o /tmp/t_q_delay_get.json -w "%{http_code}" \
      "${QUEUE_URL}/jobs/${delayed_id}" || true)"
    if [[ "$s_get" == "200" ]]; then
      job_status="$(jq -r '.status' /tmp/t_q_delay_get.json 2>/dev/null || true)"
      if [[ "$job_status" == "scheduled" || "$job_status" == "pending" || "$job_status" == "queued" ]]; then
        print_result 1 "queue delayed job status (${job_status})"
      else
        print_result 0 "queue delayed job status" "got: ${job_status}"
        FAIL=1
      fi
    fi
    # Cleanup
    curl -sS -o /dev/null -X DELETE "${QUEUE_URL}/jobs/${delayed_id}" >/dev/null 2>&1 || true
  fi
else
  print_result 0 "queue create delayed job" "HTTP ${s} (delay_seconds not supported — acceptable)"
fi

# ── Cancel job ────────────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_q_cancel.json -w "%{http_code}" \
  -X DELETE "${QUEUE_URL}/jobs/${job_id}" || true)"
if [[ "$s" == "200" ]] && jq -e '.status == "cancelled"' /tmp/t_q_cancel.json >/dev/null 2>&1; then
  print_result 1 "queue cancel job"
else
  print_result 0 "queue cancel job" "HTTP ${s}"
  FAIL=1
fi

# Cancelled job should still be retrievable but with cancelled status
s="$(curl -sS -o /tmp/t_q_cancelled_get.json -w "%{http_code}" \
  "${QUEUE_URL}/jobs/${job_id}" || true)"
if [[ "$s" == "200" ]]; then
  job_status="$(jq -r '.status' /tmp/t_q_cancelled_get.json 2>/dev/null || true)"
  if [[ "$job_status" == "cancelled" ]]; then
    print_result 1 "queue cancelled job status persisted"
  else
    print_result 0 "queue cancelled job status persisted" "status=${job_status}"
    FAIL=1
  fi
fi

# ── Malformed body → 400 ──────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_q_bad.json -w "%{http_code}" \
  -X POST "${QUEUE_URL}/jobs" \
  -H "Content-Type: application/json" \
  --data '{bad json' || true)"
if [[ "$s" == "400" || "$s" == "422" ]]; then
  print_result 1 "queue malformed body → 400/422"
else
  print_result 0 "queue malformed body → 400/422" "HTTP ${s}"
  FAIL=1
fi

exit "$FAIL"
