#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0
PAYLOAD='{"table":"users","operation":"select","limit":1}'

status1="$(curl -sS -D /tmp/platform_db_q1_headers.txt -o /tmp/platform_db_q1_body.json -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/db/query" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data "${PAYLOAD}")"

if [[ "$status1" == "200" ]] && jq -e 'type == "object"' /tmp/platform_db_q1_body.json >/dev/null; then
  print_result 1 "db query"
else
  print_result 0 "db query" "status=${status1}"
  FAIL=1
fi

status2="$(curl -sS -D /tmp/platform_db_q2_headers.txt -o /tmp/platform_db_q2_body.json -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/db/query" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data "${PAYLOAD}")"

cache1="$(grep -i '^x-cache:' /tmp/platform_db_q1_headers.txt | awk '{print toupper($2)}' | tr -d '\r' || true)"
cache2="$(grep -i '^x-cache:' /tmp/platform_db_q2_headers.txt | awk '{print toupper($2)}' | tr -d '\r' || true)"

if [[ "$status1" == "200" && "$status2" == "200" && "$cache1" == "MISS" && "$cache2" == "HIT" ]]; then
  print_result 1 "cache behavior"
else
  print_result 0 "cache behavior" "x-cache1=${cache1:-none}, x-cache2=${cache2:-none}"
  FAIL=1
fi

invalid_status="$(curl -sS -o /tmp/platform_db_invalid_body.txt -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/db/query" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data '{"table":')"

if [[ "$invalid_status" == "400" ]]; then
  print_result 1 "validation"
else
  print_result 0 "validation" "expected 400 got ${invalid_status}"
  FAIL=1
fi

exit "$FAIL"
