#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

status="$(curl -sS -o /tmp/platform_fn_body.json -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/${FUNCTION_NAME}" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data '{"message":"hello"}')"

if [[ "$status" == "200" ]] && jq -e 'has("duration_ms") or has("result") or ((.data // {}) | has("duration_ms"))' /tmp/platform_fn_body.json >/dev/null; then
  print_result 1 "functions"
  exit 0
fi

print_result 0 "functions" "status=${status}"
exit 1
