#!/usr/bin/env bash
# server_test.sh — monolithic server mount and DX tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0
SERVER_BASE="$(server_base_url)"

# ── Health ────────────────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_srv_health.json -w "%{http_code}" "${SERVER_BASE}/health" || true)"
assert_status_and_jq "200" "$s" '.status == "ok"' /tmp/t_srv_health.json "server health" FAIL

# ── /flux/api mount ───────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_srv_api_health.json -w "%{http_code}" "${SERVER_BASE}/flux/api/health" || true)"
assert_status_and_jq "200" "$s" '.status == "ok"' /tmp/t_srv_api_health.json "server api mount health" FAIL

# API version via /flux/api
s="$(curl -sS -o /tmp/t_srv_api_version.json -w "%{http_code}" "${SERVER_BASE}/flux/api/version" || true)"
assert_status_and_jq "200" "$s" '.service == "api"' /tmp/t_srv_api_version.json "server api version" FAIL

# ── Dev invoke endpoint ───────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_srv_dev_invoke.json -w "%{http_code}" \
  -X POST "${SERVER_BASE}/flux/dev/invoke/${FUNCTION_NAME}" \
  -H "Content-Type: application/json" \
  --data '{"message":"server-dev-invoke"}' || true)"

if [[ "$s" == "200" ]]; then
  print_result 1 "server dev invoke"
  assert_jq 'type == "object"' /tmp/t_srv_dev_invoke.json "server dev invoke response is object" FAIL
else
  print_result 0 "server dev invoke" "HTTP ${s}"
  FAIL=1
fi

# Dev invoke unknown function → 404 or 500 (not 200)
s="$(curl -sS -o /tmp/t_srv_dev_invoke_404.json -w "%{http_code}" \
  -X POST "${SERVER_BASE}/flux/dev/invoke/__nonexistent_$(unique_suffix)__" \
  -H "Content-Type: application/json" \
  --data '{}' || true)"
if [[ "$s" == "404" || "$s" == "500" || "$s" == "502" ]]; then
  print_result 1 "server dev invoke unknown → error (${s})"
else
  print_result 0 "server dev invoke unknown → error" "HTTP ${s}"
  FAIL=1
fi

# ── Dashboard static mount ────────────────────────────────────────────────────

s_dash="$(curl -sS -o /tmp/t_srv_dash.html -w "%{http_code}" \
  "${SERVER_BASE}/flux" || true)"
if [[ "$s_dash" == "200" || "$s_dash" == "301" || "$s_dash" == "302" ]]; then
  print_result 1 "server dashboard mount (${s_dash})"
else
  print_result 0 "server dashboard mount" "HTTP ${s_dash}"
  FAIL=1
fi

# ── Response headers ──────────────────────────────────────────────────────────

s="$(curl -sS -D /tmp/t_srv_hdr.txt -o /tmp/t_srv_hdr_body.json -w "%{http_code}" \
  "${SERVER_BASE}/flux/api/health" || true)"
if [[ "$s" == "200" ]]; then
  ct="$(http_header_value /tmp/t_srv_hdr.txt content-type || true)"
  if printf '%s' "$ct" | grep -qi 'application/json'; then
    print_result 1 "server api response content-type json"
  else
    print_result 0 "server api response content-type json" "got: ${ct:-missing}"
    FAIL=1
  fi
fi

# ── Internal bundle endpoint (service-token gated) ───────────────────────────

# Without token → 401
s="$(curl -sS -o /tmp/t_srv_bundle_no_token.json -w "%{http_code}" \
  "${SERVER_BASE}/flux/api/internal/bundle?function_id=${FUNCTION_NAME}" || true)"
assert_status "401" "$s" "server internal bundle no-token → 401" FAIL

# With token → 200 or 404
int_token="$(api_internal_service_token)"
s="$(curl -sS -o /tmp/t_srv_bundle.json -w "%{http_code}" \
  -H "X-Service-Token: ${int_token}" \
  "${SERVER_BASE}/flux/api/internal/bundle?function_id=${FUNCTION_NAME}" || true)"
if [[ "$s" == "200" || "$s" == "404" ]]; then
  print_result 1 "server internal bundle with token → ${s}"
  if [[ "$s" == "200" ]]; then
    assert_jq 'has("deployment_id") and has("runtime")' \
      /tmp/t_srv_bundle.json "server bundle response schema" FAIL
  fi
else
  print_result 0 "server internal bundle with token" "HTTP ${s}"
  FAIL=1
fi

# ── /readiness endpoint ───────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_srv_ready.json -w "%{http_code}" "${SERVER_BASE}/readiness" || true)"
if [[ "$s" == "200" || "$s" == "503" ]]; then
  assert_jq 'has("status")' /tmp/t_srv_ready.json "server readiness has status" FAIL
else
  print_result 0 "server readiness" "HTTP ${s}"
  FAIL=1
fi

exit "$FAIL"
