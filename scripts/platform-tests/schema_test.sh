#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0

status="$(curl -sS -D /tmp/platform_schema_headers.txt -o /tmp/platform_schema_body.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/schema/graph")"

if [[ "$status" == "200" ]] && jq -e '((.data // .) | has("tables") and has("columns") and has("relationships"))' /tmp/platform_schema_body.json >/dev/null; then
  print_result 1 "schema graph"
else
  print_result 0 "schema graph" "status=${status}"
  FAIL=1
fi

sdk_status="$(curl -sS -D /tmp/platform_sdk_headers.txt -o /tmp/platform_sdk_body.ts -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/sdk/typescript")"

if [[ "$sdk_status" == "200" ]] \
  && grep -iq '^x-schema-hash:' /tmp/platform_sdk_headers.txt \
  && grep -q 'createClient' /tmp/platform_sdk_body.ts; then
  print_result 1 "sdk generation"
else
  print_result 0 "sdk generation" "status=${sdk_status}"
  FAIL=1
fi

exit "$FAIL"
