#!/usr/bin/env bash
# schema_test.sh — comprehensive schema & SDK tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0

# ── Schema graph ──────────────────────────────────────────────────────────────

s="$(api_get "/schema/graph" /tmp/t_schema.json /tmp/t_schema_hdr.txt)"
assert_status_and_jq "200" "$s" \
  '((.data // .) | has("tables") and has("columns") and has("relationships"))' \
  /tmp/t_schema.json "schema graph" FAIL

# Tables must be an array
assert_jq '((.data // .) | .tables | type == "array")' \
  /tmp/t_schema.json "schema tables array" FAIL

# Columns must be an array
assert_jq '((.data // .) | .columns | type == "array")' \
  /tmp/t_schema.json "schema columns array" FAIL

# Relationships must be an array
assert_jq '((.data // .) | .relationships | type == "array")' \
  /tmp/t_schema.json "schema relationships array" FAIL

# Each table entry should have at minimum a name field
tables_ok="$(jq -e '((.data // .) | .tables | all(has("name") or has("table_name") or (type=="string")))' \
  /tmp/t_schema.json 2>/dev/null && echo 1 || echo 0)"
if [[ "$tables_ok" == "1" ]]; then
  print_result 1 "schema table entries have name"
else
  print_result 0 "schema table entries have name"
  FAIL=1
fi

# Schema must not be accessible without auth
s_noauth="$(curl -sS -o /tmp/t_schema_noauth.json -w "%{http_code}" \
  "${API_URL}/schema/graph" || true)"
assert_status "401" "$s_noauth" "schema requires auth" FAIL

# ── SDK TypeScript ────────────────────────────────────────────────────────────

s="$(curl -sS -D /tmp/t_sdk_hdr.txt -o /tmp/t_sdk.ts -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/sdk/typescript" || true)"

if [[ "$s" == "200" ]]; then
  print_result 1 "sdk typescript 200"

  # SDK must contain createClient or similar export
  if grep -q 'createClient\|FluxbaseClient\|export function\|export const\|export class' /tmp/t_sdk.ts; then
    print_result 1 "sdk typescript exports present"
  else
    print_result 0 "sdk typescript exports present" "no exports found"
    FAIL=1
  fi

  # x-schema-hash header
  if grep -iq '^x-schema-hash:' /tmp/t_sdk_hdr.txt; then
    print_result 1 "sdk x-schema-hash header"
    # Call again — hash should be stable
    s2="$(curl -sS -D /tmp/t_sdk2_hdr.txt -o /tmp/t_sdk2.ts -w "%{http_code}" \
      -H "Authorization: Bearer ${TOKEN}" \
      -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
      -H "X-Fluxbase-Project: ${PROJECT_ID}" \
      "${API_URL}/sdk/typescript" || true)"
    hash1="$(http_header_value /tmp/t_sdk_hdr.txt x-schema-hash || true)"
    hash2="$(http_header_value /tmp/t_sdk2_hdr.txt x-schema-hash || true)"
    if [[ -n "$hash1" && "$hash1" == "$hash2" ]]; then
      print_result 1 "sdk schema-hash stable across calls"
    else
      print_result 0 "sdk schema-hash stable across calls" "hash1=${hash1} hash2=${hash2}"
      FAIL=1
    fi
  else
    print_result 0 "sdk x-schema-hash header" "header missing"
    FAIL=1
  fi

  # Content-Type should be text/typescript or application/typescript or text/plain
  ct="$(http_header_value /tmp/t_sdk_hdr.txt content-type || true)"
  if printf '%s' "$ct" | grep -qi 'typescript\|text/plain\|text/'; then
    print_result 1 "sdk content-type text"
  else
    print_result 0 "sdk content-type text" "got: ${ct:-missing}"
    FAIL=1
  fi
else
  print_result 0 "sdk typescript" "HTTP ${s}"
  FAIL=1
fi

# ── Spec endpoint ─────────────────────────────────────────────────────────────

s="$(api_get "/spec" /tmp/t_spec.json)"
assert_status_and_jq "200" "$s" \
  'has("functions") and has("routes") and has("instructions")' \
  /tmp/t_spec.json "spec endpoint" FAIL

# instructions must be an object or string
assert_jq '.instructions | type == "object" or type == "string" or type == "array"' \
  /tmp/t_spec.json "spec instructions type" FAIL

# ── Schema push (dry-run when available) ─────────────────────────────────────

# Check if push endpoint exists
s="$(curl -sS -o /tmp/t_schema_push.json -w "%{http_code}" \
  -X POST \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data '{"dry_run":true}' \
  "${API_URL}/schema/push" || true)"
if [[ "$s" == "200" || "$s" == "204" || "$s" == "202" ]]; then
  print_result 1 "schema push dry-run"
elif [[ "$s" == "404" || "$s" == "405" ]]; then
  print_result 1 "schema push not implemented (acceptable)"
else
  print_result 0 "schema push dry-run" "HTTP ${s}"
  FAIL=1
fi

exit "$FAIL"
