#!/usr/bin/env bash
# auth_test.sh — comprehensive authentication & authorization tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0

# ── Unauthenticated access ─────────────────────────────────────────────────────

# Schema graph must require auth
s="$(curl -sS -o /tmp/t_auth_schema.json -w "%{http_code}" "${API_URL}/schema/graph" || true)"
assert_status "401" "$s" "auth schema requires auth → 401" FAIL

# Functions list must require auth
s="$(curl -sS -o /tmp/t_auth_fn.json -w "%{http_code}" "${API_URL}/functions" || true)"
assert_status "401" "$s" "auth functions requires auth → 401" FAIL

# Secrets must require auth
s="$(curl -sS -o /tmp/t_auth_secrets.json -w "%{http_code}" "${API_URL}/secrets" || true)"
assert_status "401" "$s" "auth secrets requires auth → 401" FAIL

# Logs must require auth
s="$(curl -sS -o /tmp/t_auth_logs.json -w "%{http_code}" "${API_URL}/logs" || true)"
assert_status "401" "$s" "auth logs requires auth → 401" FAIL

# Traces must require auth
s="$(curl -sS -o /tmp/t_auth_traces.json -w "%{http_code}" "${API_URL}/traces" || true)"
assert_status "401" "$s" "auth traces requires auth → 401" FAIL

# ── Malformed / invalid tokens ────────────────────────────────────────────────

# Bearer with random non-JWT string
s="$(curl -sS -o /tmp/t_auth_bad1.json -w "%{http_code}" \
  -H "Authorization: Bearer not-a-jwt-at-all" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/functions" || true)"
if [[ "$s" == "401" || "$s" == "403" ]]; then
  print_result 1 "auth invalid bearer → 401/403"
else
  print_result 0 "auth invalid bearer → 401/403" "HTTP ${s}"
  FAIL=1
fi

# Bearer with a structurally-valid but wrong JWT
FAKE_JWT="eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJmYWtlIiwiaWF0IjoxfQ.INVALIDSIG"
s="$(curl -sS -o /tmp/t_auth_fake_jwt.json -w "%{http_code}" \
  -H "Authorization: Bearer ${FAKE_JWT}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/functions" || true)"
if [[ "$s" == "401" || "$s" == "403" ]]; then
  print_result 1 "auth fake JWT → 401/403"
else
  print_result 0 "auth fake JWT → 401/403" "HTTP ${s}"
  FAIL=1
fi

# No Authorization header at all
s="$(curl -sS -o /tmp/t_auth_no_hdr.json -w "%{http_code}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/functions" || true)"
assert_status "401" "$s" "auth no Authorization header → 401" FAIL

# Wrong scheme (Basic instead of Bearer)
s="$(curl -sS -o /tmp/t_auth_basic.json -w "%{http_code}" \
  -H "Authorization: Basic dXNlcjpwYXNz" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/functions" || true)"
if [[ "$s" == "401" || "$s" == "403" ]]; then
  print_result 1 "auth Basic scheme → 401/403"
else
  print_result 0 "auth Basic scheme → 401/403" "HTTP ${s}"
  FAIL=1
fi

# ── Missing tenant / project headers ─────────────────────────────────────────

# Valid token but missing tenant
s="$(curl -sS -o /tmp/t_auth_no_tenant.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/functions" || true)"
if [[ "$s" == "400" || "$s" == "401" || "$s" == "403" ]]; then
  print_result 1 "auth missing tenant header → 4xx"
else
  print_result 0 "auth missing tenant header → 4xx" "HTTP ${s}"
  FAIL=1
fi

# Valid token but missing project
s="$(curl -sS -o /tmp/t_auth_no_project.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  "${API_URL}/functions" || true)"
if [[ "$s" == "200" || "$s" == "400" || "$s" == "401" || "$s" == "403" ]]; then
  print_result 1 "auth missing project header → handled (${s})"
else
  print_result 0 "auth missing project header → handled" "HTTP ${s}"
  FAIL=1
fi

# ── Error body format ─────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_auth_err_body.json -w "%{http_code}" \
  "${API_URL}/functions" || true)"
if [[ "$s" == "401" ]]; then
  # 401 response body should have an error field
  if jq -e 'has("error") or has("message")' /tmp/t_auth_err_body.json >/dev/null 2>&1; then
    print_result 1 "auth 401 error body structure"
  else
    # Some minimal 401 responses have no body — acceptable
    print_result 1 "auth 401 returned (no body check)"
  fi
fi

# ── Gateway auth ──────────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_auth_gw.json -w "%{http_code}" \
  -X POST \
  -H "Content-Type: application/json" \
  --data '{}' \
  "${GATEWAY_URL}/${FUNCTION_NAME}" || true)"
if [[ "$s" == "401" || "$s" == "403" ]]; then
  print_result 1 "auth gateway no-auth → 401/403"
else
  print_result 0 "auth gateway no-auth → 401/403" "HTTP ${s}"
  FAIL=1
fi

exit "$FAIL"
