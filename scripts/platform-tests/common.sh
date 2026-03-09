#!/usr/bin/env bash
set -euo pipefail

require_cmds() {
  for cmd in "$@"; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      echo "[FAIL] missing command: $cmd"
      exit 1
    fi
  done
}

require_env() {
  local missing=0
  for v in "$@"; do
    if [[ -z "${!v:-}" ]]; then
      echo "[FAIL] missing env var: $v"
      missing=1
    fi
  done
  if [[ "$missing" -ne 0 ]]; then
    exit 1
  fi
}

common_init() {
  require_cmds curl jq
  require_env API_URL GATEWAY_URL TOKEN TENANT_ID PROJECT_ID FUNCTION_NAME
}

auth_headers() {
  printf '%s\n' \
    "Authorization: Bearer ${TOKEN}" \
    "X-Fluxbase-Tenant: ${TENANT_ID}" \
    "X-Fluxbase-Project: ${PROJECT_ID}"
}

print_result() {
  local ok="$1"
  local name="$2"
  local detail="${3:-}"
  if [[ "$ok" == "1" ]]; then
    echo "[PASS] ${name}"
  else
    if [[ -n "$detail" ]]; then
      echo "[FAIL] ${name} (${detail})"
    else
      echo "[FAIL] ${name}"
    fi
  fi
}
