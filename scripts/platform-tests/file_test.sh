#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

payload='{"database":"main","table":"users","row_id":"test","column":"avatar"}'
status="$(curl -sS -o /tmp/platform_file_body.json -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/files/upload-url" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data "${payload}")"

if [[ "$status" == "200" ]] && jq -e 'has("upload_url") and has("object_key")' /tmp/platform_file_body.json >/dev/null; then
  print_result 1 "files"
  exit 0
fi

print_result 0 "files" "status=${status}"
exit 1
