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

http_header_value() {
  local file="$1"
  local name="$2"

  awk -v header_name="$name" '
    BEGIN { IGNORECASE = 1 }
    index(tolower($0), tolower(header_name) ":") == 1 {
      sub(/^[^:]+:[[:space:]]*/, "", $0)
      gsub(/\r/, "", $0)
      print
      exit
    }
  ' "$file"
}

wait_for_json_match() {
  local name="$1"
  local url="$2"
  local jq_expr="$3"
  local out_file="$4"
  local attempts="${5:-20}"
  local sleep_secs="${6:-1}"
  local status=""
  local i

  for ((i = 1; i <= attempts; i++)); do
    status="$(curl -sS -o "$out_file" -w "%{http_code}" \
      -H "Authorization: Bearer ${TOKEN}" \
      -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
      -H "X-Fluxbase-Project: ${PROJECT_ID}" \
      "$url" || true)"

    if [[ "$status" == "200" ]] && jq -e "$jq_expr" "$out_file" >/dev/null 2>&1; then
      return 0
    fi

    sleep "$sleep_secs"
  done

  echo "[INFO] ${name} did not satisfy jq expression after ${attempts} attempts"
  return 1
}

now_rfc3339() {
  date -u +"%Y-%m-%dT%H:%M:%SZ"
}

unique_suffix() {
  date +%s%N
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
