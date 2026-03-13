#!/usr/bin/env bash
set -euo pipefail

# ── Prerequisite checks ────────────────────────────────────────────────────────

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

# ── Auth / header helpers ──────────────────────────────────────────────────────

auth_headers() {
  printf '%s\n' \
    "Authorization: Bearer ${TOKEN}" \
    "X-Fluxbase-Tenant: ${TENANT_ID}" \
    "X-Fluxbase-Project: ${PROJECT_ID}"
}

# Read a single response header value from a curl -D dump file.
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

# Return 0 if the header exists (any value) in the dump file.
has_header() {
  local file="$1"
  local name="$2"
  local val
  val="$(http_header_value "$file" "$name")"
  [[ -n "$val" ]]
}

# ── HTTP request helper ────────────────────────────────────────────────────────

# api_get <path> <body_out> [<headers_out>]
# Returns the HTTP status code. Sends standard auth headers.
api_get() {
  local path="$1"
  local body_out="$2"
  local headers_out="${3:-/dev/null}"
  curl -sS -D "$headers_out" -o "$body_out" -w "%{http_code}" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
    -H "X-Fluxbase-Project: ${PROJECT_ID}" \
    "${API_URL}${path}" || true
}

# api_post <path> <json_data> <body_out> [<headers_out>]
api_post() {
  local path="$1"
  local data="$2"
  local body_out="$3"
  local headers_out="${4:-/dev/null}"
  curl -sS -D "$headers_out" -o "$body_out" -w "%{http_code}" \
    -X POST \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
    -H "X-Fluxbase-Project: ${PROJECT_ID}" \
    --data "$data" \
    "${API_URL}${path}" || true
}

# api_put <path> <json_data> <body_out>
api_put() {
  local path="$1"
  local data="$2"
  local body_out="$3"
  curl -sS -o "$body_out" -w "%{http_code}" \
    -X PUT \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
    -H "X-Fluxbase-Project: ${PROJECT_ID}" \
    --data "$data" \
    "${API_URL}${path}" || true
}

# api_delete <path> <body_out>
api_delete() {
  local path="$1"
  local body_out="$2"
  curl -sS -o "$body_out" -w "%{http_code}" \
    -X DELETE \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
    -H "X-Fluxbase-Project: ${PROJECT_ID}" \
    "${API_URL}${path}" || true
}

# gw_post <path> <json_data> <body_out> [<headers_out>]
gw_post() {
  local path="$1"
  local data="$2"
  local body_out="$3"
  local headers_out="${4:-/dev/null}"
  curl -sS -D "$headers_out" -o "$body_out" -w "%{http_code}" \
    -X POST \
    -H "Content-Type: application/json" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
    -H "X-Fluxbase-Project: ${PROJECT_ID}" \
    --data "$data" \
    "${GATEWAY_URL}${path}" || true
}

# ── Polling / retry helpers ────────────────────────────────────────────────────

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

# ── Assertion helpers ──────────────────────────────────────────────────────────

# assert_status <expected> <actual> <test_name> [<fail_var>]
# Prints PASS/FAIL and optionally sets a FAIL counter variable.
assert_status() {
  local expected="$1"
  local actual="$2"
  local name="$3"
  local fail_var="${4:-}"
  if [[ "$actual" == "$expected" ]]; then
    print_result 1 "$name"
  else
    print_result 0 "$name" "expected HTTP ${expected} got ${actual}"
    [[ -n "$fail_var" ]] && eval "${fail_var}=1"
  fi
}

# assert_jq <jq_expr> <json_file> <test_name> [<fail_var>]
assert_jq() {
  local expr="$1"
  local file="$2"
  local name="$3"
  local fail_var="${4:-}"
  if jq -e "$expr" "$file" >/dev/null 2>&1; then
    print_result 1 "$name"
  else
    local snippet
    snippet="$(jq -c '.' "$file" 2>/dev/null | head -c 120 || true)"
    print_result 0 "$name" "jq='${expr}' body=${snippet}"
    [[ -n "$fail_var" ]] && eval "${fail_var}=1"
  fi
}

# assert_status_and_jq <expected_status> <actual_status> <jq_expr> <json_file> <test_name> [<fail_var>]
assert_status_and_jq() {
  local expected="$1"
  local actual="$2"
  local expr="$3"
  local file="$4"
  local name="$5"
  local fail_var="${6:-}"
  if [[ "$actual" == "$expected" ]] && jq -e "$expr" "$file" >/dev/null 2>&1; then
    print_result 1 "$name"
  else
    local snippet
    snippet="$(jq -c '.' "$file" 2>/dev/null | head -c 120 || true)"
    print_result 0 "$name" "HTTP ${actual} (want ${expected}), jq='${expr}', body=${snippet}"
    [[ -n "$fail_var" ]] && eval "${fail_var}=1"
  fi
}

# assert_header_present <header_name> <dump_file> <test_name> [<fail_var>]
assert_header_present() {
  local header="$1"
  local file="$2"
  local name="$3"
  local fail_var="${4:-}"
  if has_header "$file" "$header"; then
    print_result 1 "$name"
  else
    print_result 0 "$name" "missing header: ${header}"
    [[ -n "$fail_var" ]] && eval "${fail_var}=1"
  fi
}

# ── Timing helpers ─────────────────────────────────────────────────────────────

# time_request — run curl and report the wall-clock ms
time_request() {
  curl -sS -o /dev/null -w "%{time_total}" "$@" | awk '{printf "%.0f", $1 * 1000}'
}

# ── Miscellaneous ──────────────────────────────────────────────────────────────

now_rfc3339() {
  date -u +"%Y-%m-%dT%H:%M:%SZ"
}

unique_suffix() {
  date +%s%N
}

server_base_url() {
  if [[ -n "${SERVER_URL:-}" ]]; then
    printf '%s\n' "${SERVER_URL%/}"
    return
  fi

  case "${API_URL}" in
    */flux/api) printf '%s\n' "${API_URL%/flux/api}" ;;
    *)          printf '%s\n' "${GATEWAY_URL%/}" ;;
  esac
}

api_internal_service_token() {
  printf '%s\n' "${API_INTERNAL_SERVICE_TOKEN:-${INTERNAL_SERVICE_TOKEN:-dev-service-token}}"
}

data_engine_service_token() {
  printf '%s\n' "${DATA_ENGINE_SERVICE_TOKEN:-${INTERNAL_SERVICE_TOKEN:-fluxbase_secret_token}}"
}

fetch_function_id() {
  local out_file="${1:-/tmp/platform_functions.json}"
  local status

  status="$(curl -sS -o "$out_file" -w "%{http_code}" \
    -H "Authorization: Bearer ${TOKEN}" \
    -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
    -H "X-Fluxbase-Project: ${PROJECT_ID}" \
    "${API_URL}/functions")"

  if [[ "$status" != "200" ]]; then
    return 1
  fi

  jq -er --arg name "${FUNCTION_NAME}" \
    '.functions[] | select(.name == $name) | .id' \
    "$out_file"
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
