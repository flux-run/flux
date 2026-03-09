#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

status="$(curl -sS -o /tmp/platform_logs_body.json -w "%{http_code}" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${API_URL}/logs?limit=5")"

if [[ "$status" == "200" ]] && jq -e '((.data // .).logs | type == "array")' /tmp/platform_logs_body.json >/dev/null; then
  print_result 1 "logs"
  exit 0
fi

print_result 0 "logs" "status=${status}"
exit 1
