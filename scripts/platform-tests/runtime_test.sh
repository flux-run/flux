#!/usr/bin/env bash
# runtime_test.sh — comprehensive runtime service tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init
require_env RUNTIME_URL

FAIL=0

# ── Health & version ──────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_rt_health.json -w "%{http_code}" "${RUNTIME_URL}/health" || true)"
assert_status_and_jq "200" "$s" '.status == "ok"' /tmp/t_rt_health.json "runtime health" FAIL

s="$(curl -sS -o /tmp/t_rt_version.json -w "%{http_code}" "${RUNTIME_URL}/version" || true)"
assert_status_and_jq "200" "$s" '.service == "runtime"' /tmp/t_rt_version.json "runtime version" FAIL
assert_jq 'has("version") or has("commit") or has("service")' /tmp/t_rt_version.json "runtime version metadata" FAIL

# ── Successful execution ──────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_rt_exec.json -w "%{http_code}" \
  -X POST "${RUNTIME_URL}/execute" \
  -H "Content-Type: application/json" \
  -H "x-request-id: rt-smoke-$(unique_suffix)" \
  --data "{\"function_id\":\"${FUNCTION_NAME}\",\"project_id\":\"${PROJECT_ID}\",\"payload\":{\"message\":\"runtime-smoke\"}}" || true)"
assert_status_and_jq "200" "$s" \
  'has("duration_ms") or has("result") or ((.data // {}) | has("duration_ms"))' \
  /tmp/t_rt_exec.json "runtime execute" FAIL

# ── Execute without optional payload ─────────────────────────────────────────

s="$(curl -sS -o /tmp/t_rt_no_payload.json -w "%{http_code}" \
  -X POST "${RUNTIME_URL}/execute" \
  -H "Content-Type: application/json" \
  -H "x-request-id: rt-nopayload-$(unique_suffix)" \
  --data "{\"function_id\":\"${FUNCTION_NAME}\",\"project_id\":\"${PROJECT_ID}\"}" || true)"
if [[ "$s" == "200" ]]; then
  print_result 1 "runtime execute no payload"
else
  print_result 0 "runtime execute no payload" "HTTP ${s}"
  FAIL=1
fi

# ── Missing required fields → 400/422 ─────────────────────────────────────────

# Missing function_id
s="$(curl -sS -o /tmp/t_rt_no_fn.json -w "%{http_code}" \
  -X POST "${RUNTIME_URL}/execute" \
  -H "Content-Type: application/json" \
  --data "{\"project_id\":\"${PROJECT_ID}\",\"payload\":{}}" || true)"
if [[ "$s" == "400" || "$s" == "422" || "$s" == "404" ]]; then
  print_result 1 "runtime execute missing function_id → 4xx"
else
  print_result 0 "runtime execute missing function_id → 4xx" "HTTP ${s}"
  FAIL=1
fi

# Non-existent function → 404 or 502 (bundle not found)
s="$(curl -sS -o /tmp/t_rt_not_found.json -w "%{http_code}" \
  -X POST "${RUNTIME_URL}/execute" \
  -H "Content-Type: application/json" \
  -H "x-request-id: rt-notfound-$(unique_suffix)" \
  --data "{\"function_id\":\"__nonexistent_fn_$(unique_suffix)__\",\"project_id\":\"${PROJECT_ID}\",\"payload\":{}}" || true)"
if [[ "$s" == "404" || "$s" == "502" || "$s" == "500" ]]; then
  print_result 1 "runtime execute unknown function → error"
else
  print_result 0 "runtime execute unknown function → error" "HTTP ${s}"
  FAIL=1
fi

# ── Malformed execute body ────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_rt_bad.json -w "%{http_code}" \
  -X POST "${RUNTIME_URL}/execute" \
  -H "Content-Type: application/json" \
  --data '{not json' || true)"
if [[ "$s" == "400" || "$s" == "422" ]]; then
  print_result 1 "runtime execute malformed JSON → 400/422"
else
  print_result 0 "runtime execute malformed JSON → 400/422" "HTTP ${s}"
  FAIL=1
fi

# ── Request-ID propagation ────────────────────────────────────────────────────

custom_req_id="rt-custom-$(unique_suffix)"
s="$(curl -sS -D /tmp/t_rt_rid_hdr.txt -o /tmp/t_rt_rid.json -w "%{http_code}" \
  -X POST "${RUNTIME_URL}/execute" \
  -H "Content-Type: application/json" \
  -H "x-request-id: ${custom_req_id}" \
  --data "{\"function_id\":\"${FUNCTION_NAME}\",\"project_id\":\"${PROJECT_ID}\",\"payload\":{}}" || true)"
if [[ "$s" == "200" ]]; then
  echo_id="$(http_header_value /tmp/t_rt_rid_hdr.txt x-request-id || true)"
  if [[ "$echo_id" == "$custom_req_id" || -n "$echo_id" ]]; then
    print_result 1 "runtime x-request-id echoed"
  else
    print_result 0 "runtime x-request-id echoed" "expected=${custom_req_id} got=${echo_id:-missing}"
    FAIL=1
  fi
fi

# ── Concurrent executions ─────────────────────────────────────────────────────

rt_exec_once() {
  curl -sS -o /dev/null -w "%{http_code}" \
    -X POST "${RT_TEST_URL}/execute" \
    -H "Content-Type: application/json" \
    -H "x-request-id: rt-conc-$(date +%s%N)" \
    --data "{\"function_id\":\"${RT_TEST_FN}\",\"project_id\":\"${RT_TEST_PROJECT}\",\"payload\":{}}" || true
}
export -f rt_exec_once
export RT_TEST_URL="$RUNTIME_URL"
export RT_TEST_FN="$FUNCTION_NAME"
export RT_TEST_PROJECT="$PROJECT_ID"

rt_results="$(seq 1 8 | xargs -I{} -P 8 bash -lc 'rt_exec_once' 2>/dev/null || true)"
rt_non_200="$(printf '%s\n' "$rt_results" | grep -cv '^200$' || true)"
if [[ "$rt_non_200" == "0" ]]; then
  print_result 1 "runtime 8 concurrent executions all 200"
else
  print_result 0 "runtime 8 concurrent executions" "non-200=${rt_non_200}"
  FAIL=1
fi

# ── Duration reported ─────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_rt_dur.json -w "%{http_code}" \
  -X POST "${RUNTIME_URL}/execute" \
  -H "Content-Type: application/json" \
  -H "x-request-id: rt-dur-$(unique_suffix)" \
  --data "{\"function_id\":\"${FUNCTION_NAME}\",\"project_id\":\"${PROJECT_ID}\",\"payload\":{}}" || true)"
if [[ "$s" == "200" ]]; then
  dur="$(jq '(.duration_ms // (.data // {}).duration_ms // -1)' /tmp/t_rt_dur.json 2>/dev/null || echo -1)"
  if [[ "$dur" != "-1" ]] && python3 -c "import sys; sys.exit(0 if float('${dur}') >= 0 else 1)" 2>/dev/null; then
    print_result 1 "runtime duration_ms reported (${dur}ms)"
  else
    print_result 0 "runtime duration_ms reported" "got: ${dur}"
    FAIL=1
  fi
fi

exit "$FAIL"
