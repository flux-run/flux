#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

rm -f /tmp/platform_sse_body.txt /tmp/platform_sse_headers.txt
status="$(curl -sS -N --max-time 25 -D /tmp/platform_sse_headers.txt -o /tmp/platform_sse_body.txt -w "%{http_code}" \
  -H "Accept: text/event-stream" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${GATEWAY_URL}/events/stream" || true)"

if [[ "$status" == "200" ]] && grep -q 'event: heartbeat' /tmp/platform_sse_body.txt; then
  print_result 1 "events"
  exit 0
fi

print_result 0 "events" "status=${status}"
exit 1
