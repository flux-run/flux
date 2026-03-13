#!/usr/bin/env bash
# api_test.sh — comprehensive API management endpoint tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0
SUFFIX="$(unique_suffix)"

# ── Health & version ──────────────────────────────────────────────────────────

s="$(api_get "/health" /tmp/t_api_health.json)"
assert_status_and_jq "200" "$s" '.status == "ok"' /tmp/t_api_health.json "api health" FAIL

s="$(api_get "/version" /tmp/t_api_version.json)"
assert_status_and_jq "200" "$s" '.service == "api"' /tmp/t_api_version.json "api version" FAIL

# version should include build metadata fields
assert_jq 'has("version") or has("commit") or has("service")' /tmp/t_api_version.json "api version metadata" FAIL

# ── Schema & SDK ──────────────────────────────────────────────────────────────

s="$(api_get "/schema/graph" /tmp/t_api_schema.json)"
assert_status_and_jq "200" "$s" \
  '((.data // .) | has("tables") and has("columns") and has("relationships"))' \
  /tmp/t_api_schema.json "api schema graph" FAIL

# schema tables must be an array
assert_jq '((.data // .) | .tables | type == "array")' /tmp/t_api_schema.json "api schema tables array" FAIL

s="$(api_get "/spec" /tmp/t_api_spec.json)"
assert_status_and_jq "200" "$s" \
  'has("functions") and has("routes") and has("instructions")' \
  /tmp/t_api_spec.json "api spec" FAIL

# spec functions list must be an array
assert_jq '.functions | type == "array"' /tmp/t_api_spec.json "api spec functions array" FAIL

# ── SDK TypeScript generation ──────────────────────────────────────────────────

s="$(curl -sS -D /tmp/t_sdk_headers.txt -o /tmp/t_sdk.ts -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/sdk/typescript" || true)"

if [[ "$s" == "200" ]]; then
  print_result 1 "api sdk typescript status"
  if grep -q 'createClient\|FluxbaseClient\|export' /tmp/t_sdk.ts 2>/dev/null; then
    print_result 1 "api sdk typescript content"
  else
    print_result 0 "api sdk typescript content" "missing exports"
    FAIL=1
  fi
  if grep -iq '^x-schema-hash:' /tmp/t_sdk_headers.txt; then
    print_result 1 "api sdk x-schema-hash header"
  else
    print_result 0 "api sdk x-schema-hash header" "header missing"
    FAIL=1
  fi
else
  print_result 0 "api sdk typescript status" "HTTP ${s}"
  FAIL=1
fi

# ── Internal auth protection ──────────────────────────────────────────────────

# Internal endpoint must reject requests without service token
s="$(curl -sS -o /tmp/t_api_internal_no_token.json -w "%{http_code}" \
  "${API_URL}/internal/introspect?project_id=${PROJECT_ID}&tenant_id=${TENANT_ID}" || true)"
assert_status "401" "$s" "api internal no-token → 401" FAIL

# ── Functions CRUD ────────────────────────────────────────────────────────────

# List functions — must return an array
s="$(api_get "/functions" /tmp/t_fn_list.json)"
assert_status_and_jq "200" "$s" \
  '(.functions // .data // .) | type == "array"' \
  /tmp/t_fn_list.json "api list functions" FAIL

# List functions — pagination: limit=1
s="$(curl -sS -o /tmp/t_fn_limit.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/functions?limit=1" || true)"
if [[ "$s" == "200" ]]; then
  count="$(jq '(.functions // .data // .) | length' /tmp/t_fn_limit.json 2>/dev/null || echo 99)"
  if [[ "$count" -le 1 ]]; then
    print_result 1 "api list functions limit=1"
  else
    print_result 0 "api list functions limit=1" "returned ${count} items"
    FAIL=1
  fi
else
  print_result 0 "api list functions limit=1" "HTTP ${s}"
  FAIL=1
fi

# Get specific function by name
s="$(api_get "/functions/${FUNCTION_NAME}" /tmp/t_fn_get.json)"
if [[ "$s" == "200" ]]; then
  assert_jq --arg n "$FUNCTION_NAME" \
    '((.data // .) | .name == $n or .function.name == $n)' \
    /tmp/t_fn_get.json "api get function by name" FAIL
elif [[ "$s" == "404" ]]; then
  print_result 1 "api get function by name (not found acceptable)"
else
  print_result 0 "api get function by name" "HTTP ${s}"
  FAIL=1
fi

# ── Secrets CRUD ──────────────────────────────────────────────────────────────

secret_key="TEST_SECRET_${SUFFIX}"
secret_val="test-value-${SUFFIX}"

# Create secret
s="$(api_post "/secrets" \
  "{\"key\":\"${secret_key}\",\"value\":\"${secret_val}\"}" \
  /tmp/t_secret_create.json)"
if [[ "$s" == "200" || "$s" == "201" ]]; then
  print_result 1 "api create secret"
else
  print_result 0 "api create secret" "HTTP ${s}"
  FAIL=1
fi

# List secrets — key should appear (value masked)
s="$(api_get "/secrets" /tmp/t_secret_list.json)"
if [[ "$s" == "200" ]]; then
  if jq -e --arg k "$secret_key" \
    '(.secrets // .data // .) | any(.[]; .key == $k)' \
    /tmp/t_secret_list.json >/dev/null 2>&1; then
    print_result 1 "api list secrets contains key"
  else
    print_result 0 "api list secrets contains key" "key not found"
    FAIL=1
  fi
  # Secret value must be masked / not exposed
  if jq -e --arg v "$secret_val" \
    '(.secrets // .data // .[]) | any(.[]; .value == $v)' \
    /tmp/t_secret_list.json >/dev/null 2>&1; then
    print_result 0 "api secrets value masked" "plaintext value exposed!"
    FAIL=1
  else
    print_result 1 "api secrets value masked"
  fi
else
  print_result 0 "api list secrets" "HTTP ${s}"
  FAIL=1
fi

# Update secret
s="$(api_put "/secrets/${secret_key}" \
  "{\"value\":\"updated-${SUFFIX}\"}" \
  /tmp/t_secret_update.json)"
if [[ "$s" == "200" || "$s" == "204" ]]; then
  print_result 1 "api update secret"
else
  print_result 0 "api update secret" "HTTP ${s}"
  FAIL=1
fi

# Delete secret
s="$(api_delete "/secrets/${secret_key}" /tmp/t_secret_delete.json)"
if [[ "$s" == "200" || "$s" == "204" ]]; then
  print_result 1 "api delete secret"
else
  print_result 0 "api delete secret" "HTTP ${s}"
  FAIL=1
fi

# Deleted secret must not appear in list
s="$(api_get "/secrets" /tmp/t_secret_list2.json)"
if [[ "$s" == "200" ]]; then
  if jq -e --arg k "$secret_key" \
    '(.secrets // .data // .) | any(.[]; .key == $k)' \
    /tmp/t_secret_list2.json >/dev/null 2>&1; then
    print_result 0 "api deleted secret gone" "still in list"
    FAIL=1
  else
    print_result 1 "api deleted secret gone"
  fi
fi

# Duplicate key should be idempotent or 409
s="$(api_post "/secrets" \
  "{\"key\":\"${secret_key}\",\"value\":\"x\"}" \
  /tmp/t_secret_dup.json)"
s2="$(api_post "/secrets" \
  "{\"key\":\"${secret_key}\",\"value\":\"x\"}" \
  /tmp/t_secret_dup2.json)"
# Second call may be 200/201 (upsert) or 409 (conflict) — either is fine
if [[ "$s2" == "200" || "$s2" == "201" || "$s2" == "409" ]]; then
  print_result 1 "api duplicate secret idempotent or conflict"
else
  print_result 0 "api duplicate secret idempotent or conflict" "HTTP ${s2}"
  FAIL=1
fi
# Cleanup
api_delete "/secrets/${secret_key}" /dev/null >/dev/null 2>&1 || true

# ── Routes ────────────────────────────────────────────────────────────────────

s="$(api_get "/routes" /tmp/t_routes_list.json)"
if [[ "$s" == "200" ]]; then
  assert_jq '(.routes // .data // .) | type == "array"' \
    /tmp/t_routes_list.json "api list routes" FAIL
else
  print_result 0 "api list routes" "HTTP ${s}"
  FAIL=1
fi

# ── Deployments list ──────────────────────────────────────────────────────────

s="$(api_get "/deployments" /tmp/t_deploy_list.json)"
if [[ "$s" == "200" ]]; then
  assert_jq '(.deployments // .data // .) | type == "array"' \
    /tmp/t_deploy_list.json "api list deployments" FAIL
else
  print_result 0 "api list deployments" "HTTP ${s}"
  FAIL=1
fi

# ── Logs ─────────────────────────────────────────────────────────────────────

s="$(api_get "/logs?limit=5" /tmp/t_api_logs.json)"
assert_status_and_jq "200" "$s" \
  '((.data // .).logs | type == "array")' \
  /tmp/t_api_logs.json "api logs list" FAIL

# Logs with limit=1
s="$(curl -sS -o /tmp/t_api_logs1.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/logs?limit=1" || true)"
if [[ "$s" == "200" ]]; then
  count="$(jq '((.data // .).logs) | length' /tmp/t_api_logs1.json 2>/dev/null || echo 99)"
  if [[ "$count" -le 1 ]]; then
    print_result 1 "api logs limit=1"
  else
    print_result 0 "api logs limit=1" "returned ${count} items"
    FAIL=1
  fi
else
  print_result 0 "api logs limit=1" "HTTP ${s}"
  FAIL=1
fi

# Logs — filter by level=error
s="$(curl -sS -o /tmp/t_api_logs_err.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/logs?level=error&limit=10" || true)"
if [[ "$s" == "200" ]]; then
  if jq -e '((.data // .).logs | type == "array") and
    all((.data // .).logs[]; .level == "error" or .level == null)' \
    /tmp/t_api_logs_err.json >/dev/null 2>&1; then
    print_result 1 "api logs filter level=error"
  else
    print_result 0 "api logs filter level=error" "non-error entries returned"
    FAIL=1
  fi
else
  print_result 0 "api logs filter level=error" "HTTP ${s}"
  FAIL=1
fi

# ── Error response format ─────────────────────────────────────────────────────

# Malformed JSON body → 400 with structured error
s="$(curl -sS -o /tmp/t_api_badjson.json -w "%{http_code}" \
  -X POST \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data '{broken' \
  "${API_URL}/secrets" || true)"
if [[ "$s" == "400" ]]; then
  assert_jq 'has("error") or has("message")' /tmp/t_api_badjson.json "api error response has error field" FAIL
else
  # Some implementations return 422 or 415 for bad JSON — acceptable
  if [[ "$s" == "422" || "$s" == "415" || "$s" == "400" ]]; then
    print_result 1 "api bad json → 4xx"
  else
    print_result 0 "api bad json → 4xx" "HTTP ${s}"
    FAIL=1
  fi
fi

# Empty Authorization header → 401
s="$(curl -sS -o /tmp/t_api_no_auth.json -w "%{http_code}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/functions" || true)"
assert_status "401" "$s" "api no-auth → 401" FAIL

# Missing tenant header — behaviour may be 400 or 401 (not 200)
s="$(curl -sS -o /tmp/t_api_no_tenant.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  "${API_URL}/functions" || true)"
if [[ "$s" == "400" || "$s" == "401" || "$s" == "403" ]]; then
  print_result 1 "api missing tenant header → 4xx"
else
  print_result 0 "api missing tenant header → 4xx" "HTTP ${s}"
  FAIL=1
fi

# ── Pagination cursor / total ─────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_fn_page.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/functions?limit=2&offset=0" || true)"
if [[ "$s" == "200" ]]; then
  print_result 1 "api functions pagination offset=0"
  # total or has_more field expected
  if jq -e 'has("total") or has("has_more") or has("next_cursor")' /tmp/t_fn_page.json >/dev/null 2>&1; then
    print_result 1 "api functions pagination metadata"
  else
    print_result 0 "api functions pagination metadata" "no total/has_more/next_cursor"
    FAIL=1
  fi
else
  print_result 0 "api functions pagination offset=0" "HTTP ${s}"
  FAIL=1
fi

# ── Traces list ───────────────────────────────────────────────────────────────

s="$(api_get "/traces?limit=5" /tmp/t_traces_list.json)"
if [[ "$s" == "200" ]]; then
  assert_jq '(.traces // .data // .) | type == "array"' \
    /tmp/t_traces_list.json "api traces list" FAIL
else
  print_result 0 "api traces list" "HTTP ${s}"
  FAIL=1
fi

exit "$FAIL"
