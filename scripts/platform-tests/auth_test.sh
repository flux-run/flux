#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

status="$(curl -sS -o /tmp/platform_auth_body.json -w "%{http_code}" "${API_URL}/schema/graph")"
if [[ "$status" == "401" ]]; then
  print_result 1 "auth protection"
  exit 0
fi

print_result 0 "auth protection" "expected 401 got ${status}"
exit 1
