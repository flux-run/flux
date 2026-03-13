#!/usr/bin/env bash
# gateway_test.sh — comprehensive gateway tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0

# ── Health & readiness ────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_gw_health.json -w "%{http_code}" "${GATEWAY_URL}/health" || true)"
assert_status_and_jq "200" "$s" '.status == "ok"' /tmp/t_gw_health.json "gateway health" FAIL

s="$(curl -sS -o /tmp/t_gw_ready.json -w "%{http_code}" "${GATEWAY_URL}/readiness" || true)"
if [[ "$s" == "200" || "$s" == "503" ]]; then
  assert_jq 'has("status")' /tmp/t_gw_ready.json "gateway readiness has status" FAIL
else
  print_result 0 "gateway readiness" "HTTP ${s}"
  FAIL=1
fi

# ── Successful invocation ─────────────────────────────────────────────────────

s="$(gw_post "/${FUNCTION_NAME}" '{"message":"gateway-smoke"}' \
  /tmp/t_gw_invoke.json /tmp/t_gw_invoke_hdr.txt)"
assert_status "200" "$s" "gateway invoke 200" FAIL
assert_header_present "x-request-id" /tmp/t_gw_invoke_hdr.txt "gateway invoke x-request-id" FAIL

# request-id must be a non-empty string
req_id="$(http_header_value /tmp/t_gw_invoke_hdr.txt x-request-id || true)"
if [[ -n "$req_id" ]]; then
  print_result 1 "gateway request-id non-empty"
else
  print_result 0 "gateway request-id non-empty"
  FAIL=1
fi

# Two calls must produce different request-ids
s2="$(gw_post "/${FUNCTION_NAME}" '{"message":"gateway-smoke-2"}' \
  /tmp/t_gw_invoke2.json /tmp/t_gw_invoke2_hdr.txt)"
req_id2="$(http_header_value /tmp/t_gw_invoke2_hdr.txt x-request-id || true)"
if [[ -n "$req_id2" && "$req_id" != "$req_id2" ]]; then
  print_result 1 "gateway request-id unique per call"
else
  print_result 0 "gateway request-id unique per call" "id1=${req_id} id2=${req_id2:-missing}"
  FAIL=1
fi

# ── Route not found ────────────────────────────────────────────────────────────

s="$(gw_post "/__does_not_exist__" '{}' /tmp/t_gw_404.json)"
assert_status_and_jq "404" "$s" \
  '.error == "route_not_found"' \
  /tmp/t_gw_404.json "gateway unknown route → 404 route_not_found" FAIL

# ── Auth failures ─────────────────────────────────────────────────────────────

# No Authorization header → 401
s="$(curl -sS -o /tmp/t_gw_no_auth.json -w "%{http_code}" \
  -X POST \
  -H "Content-Type: application/json" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data '{}' \
  "${GATEWAY_URL}/${FUNCTION_NAME}" || true)"
assert_status "401" "$s" "gateway no auth → 401" FAIL

# Malformed bearer token → 401
s="$(curl -sS -o /tmp/t_gw_bad_token.json -w "%{http_code}" \
  -X POST \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer THIS_IS_NOT_A_REAL_TOKEN" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data '{}' \
  "${GATEWAY_URL}/${FUNCTION_NAME}" || true)"
if [[ "$s" == "401" || "$s" == "403" ]]; then
  print_result 1 "gateway invalid token → 401/403"
else
  print_result 0 "gateway invalid token → 401/403" "HTTP ${s}"
  FAIL=1
fi

# ── Request body validation ───────────────────────────────────────────────────

# Malformed JSON → 400 (or gateway may pass through to function)
s="$(curl -sS -o /tmp/t_gw_badjson.json -w "%{http_code}" \
  -X POST \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data '{broken json' \
  "${GATEWAY_URL}/${FUNCTION_NAME}" || true)"
if [[ "$s" == "400" || "$s" == "422" || "$s" == "200" ]]; then
  print_result 1 "gateway malformed JSON handled (${s})"
else
  print_result 0 "gateway malformed JSON handled" "HTTP ${s}"
  FAIL=1
fi

# Empty body (no Content-Type) — gateway must not crash
s="$(curl -sS -o /tmp/t_gw_empty.json -w "%{http_code}" \
  -X POST \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${GATEWAY_URL}/${FUNCTION_NAME}" || true)"
if [[ "$s" == "200" || "$s" == "400" || "$s" == "422" ]]; then
  print_result 1 "gateway empty body handled (${s})"
else
  print_result 0 "gateway empty body handled" "HTTP ${s}"
  FAIL=1
fi

# ── HTTP method guard ─────────────────────────────────────────────────────────

# GET on function route — framework functions are POST-only
s="$(curl -sS -o /tmp/t_gw_get.json -w "%{http_code}" \
  -X GET \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${GATEWAY_URL}/${FUNCTION_NAME}" || true)"
if [[ "$s" == "405" || "$s" == "404" || "$s" == "200" ]]; then
  print_result 1 "gateway GET on function route handled (${s})"
else
  print_result 0 "gateway GET on function route handled" "HTTP ${s}"
  FAIL=1
fi

# ── Large payload ─────────────────────────────────────────────────────────────

# 2 MB payload — must not crash the gateway
large_val="$(python3 -c "print('x'*2000000)")"
s="$(curl -sS -o /tmp/t_gw_large.json -w "%{http_code}" \
  -X POST \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data "{\"data\":\"${large_val}\"}" \
  "${GATEWAY_URL}/${FUNCTION_NAME}" || true)"
if [[ "$s" == "413" || "$s" == "400" || "$s" == "200" ]]; then
  print_result 1 "gateway large payload handled (${s})"
else
  print_result 0 "gateway large payload handled" "HTTP ${s}"
  FAIL=1
fi

# ── Concurrent requests ────────────────────────────────────────────────────────

invoke_once() {
  curl -sS -o /dev/null -w "%{http_code}" \
    -X POST \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer ${GATEWAY_TEST_TOKEN}" \
    -H "X-Fluxbase-Tenant: ${GATEWAY_TEST_TENANT}" \
    -H "X-Fluxbase-Project: ${GATEWAY_TEST_PROJECT}" \
    --data '{"message":"concurrent"}' \
    "${GATEWAY_TEST_URL}/${GATEWAY_TEST_FN}" || true
}
export -f invoke_once
export GATEWAY_TEST_TOKEN="$TOKEN"
export GATEWAY_TEST_TENANT="$TENANT_ID"
export GATEWAY_TEST_PROJECT="$PROJECT_ID"
export GATEWAY_TEST_URL="$GATEWAY_URL"
export GATEWAY_TEST_FN="$FUNCTION_NAME"

concurrent_results="$(seq 1 10 | xargs -I{} -P 10 bash -lc 'invoke_once' 2>/dev/null || true)"
non_200="$(printf '%s\n' "$concurrent_results" | grep -cv '^200$' || true)"
if [[ "$non_200" == "0" ]]; then
  print_result 1 "gateway 10 concurrent requests all 200"
else
  print_result 0 "gateway 10 concurrent requests" "non-200 count=${non_200}"
  FAIL=1
fi

# ── Response headers ──────────────────────────────────────────────────────────

# Content-Type must be application/json for function responses
s="$(gw_post "/${FUNCTION_NAME}" '{"message":"headers-check"}' \
  /tmp/t_gw_ct.json /tmp/t_gw_ct_hdr.txt)"
if [[ "$s" == "200" ]]; then
  ct="$(http_header_value /tmp/t_gw_ct_hdr.txt content-type || true)"
  if printf '%s' "$ct" | grep -qi 'application/json'; then
    print_result 1 "gateway response content-type json"
  else
    print_result 0 "gateway response content-type json" "got: ${ct:-missing}"
    FAIL=1
  fi
fi

exit "$FAIL"
