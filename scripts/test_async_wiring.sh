#!/bin/bash

set -euo pipefail

# Deterministic async wiring test
# Verifies: Client -> Gateway -> Queue -> Worker -> Runtime (via job completion)
#
# Required env vars:
#   API_BASE_URL        e.g. https://api.fluxbase.co
#   GATEWAY_BASE_URL    e.g. https://run.fluxbase.co
#   AUTH_TOKEN          Bearer token for control plane route creation
#   TENANT_ID           UUID for X-Fluxbase-Tenant header
#   PROJECT_ID          UUID for X-Fluxbase-Project header
#   TENANT_SLUG         tenant slug for x-tenant gateway header
#   FUNCTION_ID         UUID to attach route to
#   DATABASE_URL        Postgres connection string
#
# Optional env vars:
#   ROUTE_PATH                  default: /send-email
#   ROUTE_METHOD                default: POST
#   TEST_PAYLOAD                default: {"email":"test@example.com"}
#   POLL_TIMEOUT_SECONDS        default: 30
#   POLL_INTERVAL_SECONDS       default: 1
#   RUNTIME_LOG_CHECK_CMD       shell command; should print logs including FUNCTION_ID
#   KEEP_ROUTE                  set to 1 to skip route cleanup

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "ERROR: required command not found: $1"
    exit 1
  fi
}

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    echo "ERROR: required env var missing: $name"
    exit 1
  fi
}

json_escape() {
  jq -c . <<<"$1"
}

psql_value() {
  local query="$1"
  psql "$DATABASE_URL" -X -A -t -c "$query" | tr -d '[:space:]'
}

log_step() {
  echo
  echo "==> $1"
}

API_BASE_URL="${API_BASE_URL:-}"
GATEWAY_BASE_URL="${GATEWAY_BASE_URL:-}"
AUTH_TOKEN="${AUTH_TOKEN:-}"
TENANT_ID="${TENANT_ID:-}"
PROJECT_ID="${PROJECT_ID:-}"
TENANT_SLUG="${TENANT_SLUG:-}"
FUNCTION_ID="${FUNCTION_ID:-}"
DATABASE_URL="${DATABASE_URL:-}"

ROUTE_PATH="${ROUTE_PATH:-/send-email}"
ROUTE_METHOD="${ROUTE_METHOD:-POST}"
TEST_PAYLOAD="${TEST_PAYLOAD:-{\"email\":\"test@example.com\"}}"
POLL_TIMEOUT_SECONDS="${POLL_TIMEOUT_SECONDS:-30}"
POLL_INTERVAL_SECONDS="${POLL_INTERVAL_SECONDS:-1}"
RUNTIME_LOG_CHECK_CMD="${RUNTIME_LOG_CHECK_CMD:-}"
KEEP_ROUTE="${KEEP_ROUTE:-0}"

require_cmd curl
require_cmd jq
require_cmd psql

require_env API_BASE_URL
require_env GATEWAY_BASE_URL
require_env AUTH_TOKEN
require_env TENANT_ID
require_env PROJECT_ID
require_env TENANT_SLUG
require_env FUNCTION_ID
require_env DATABASE_URL

if ! jq -e . >/dev/null 2>&1 <<<"$TEST_PAYLOAD"; then
  echo "ERROR: TEST_PAYLOAD is not valid JSON"
  exit 1
fi

ROUTE_ID=""
JOB_ID=""

cleanup() {
  if [[ -n "$ROUTE_ID" && "$KEEP_ROUTE" != "1" ]]; then
    log_step "Cleanup route $ROUTE_ID"
    curl -sS -o /dev/null -w "%{http_code}" -X DELETE "${API_BASE_URL}/routes/${ROUTE_ID}" \
      -H "Authorization: Bearer ${AUTH_TOKEN}" \
      -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
      -H "X-Fluxbase-Project: ${PROJECT_ID}" \
      >/dev/null || true
  fi
}
trap cleanup EXIT

log_step "Preflight health checks"
for url in "${API_BASE_URL}/health" "${GATEWAY_BASE_URL}/health"; do
  code=$(curl -sS -o /dev/null -w "%{http_code}" "$url")
  if [[ "$code" != "200" ]]; then
    echo "ERROR: health check failed for $url (HTTP $code)"
    exit 1
  fi
  echo "OK: $url"
done

log_step "Create async route"
create_payload=$(jq -n \
  --arg path "$ROUTE_PATH" \
  --arg method "$ROUTE_METHOD" \
  --arg function_id "$FUNCTION_ID" \
  '{path:$path,method:$method,function_id:$function_id,is_async:true,auth_type:"none",cors_enabled:false,rate_limit:null}')

create_resp_file=$(mktemp)
create_code=$(curl -sS -o "$create_resp_file" -w "%{http_code}" -X POST "${API_BASE_URL}/routes" \
  -H "Authorization: Bearer ${AUTH_TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  -H "Content-Type: application/json" \
  -d "$create_payload")

if [[ "$create_code" != "200" ]]; then
  echo "ERROR: route creation failed (HTTP $create_code)"
  cat "$create_resp_file"
  exit 1
fi

ROUTE_ID=$(jq -r '.data.id // empty' "$create_resp_file")
if [[ -z "$ROUTE_ID" || "$ROUTE_ID" == "null" ]]; then
  echo "ERROR: route_id missing in control plane response"
  cat "$create_resp_file"
  exit 1
fi

route_async=$(jq -r '.data.is_async // empty' "$create_resp_file")
if [[ "$route_async" != "true" ]]; then
  echo "ERROR: created route is_async is not true"
  cat "$create_resp_file"
  exit 1
fi

echo "OK: route created: $ROUTE_ID"

log_step "Invoke gateway async endpoint"
gw_resp_file=$(mktemp)
gw_code=$(curl -sS -o "$gw_resp_file" -w "%{http_code}" -X "$ROUTE_METHOD" "${GATEWAY_BASE_URL}${ROUTE_PATH}" \
  -H "Content-Type: application/json" \
  -H "x-tenant: ${TENANT_SLUG}" \
  -d "$TEST_PAYLOAD")

if [[ "$gw_code" != "202" ]]; then
  echo "ERROR: gateway did not return 202 (got $gw_code)"
  cat "$gw_resp_file"
  exit 1
fi

JOB_ID=$(jq -r '.job_id // empty' "$gw_resp_file")
queued_status=$(jq -r '.status // empty' "$gw_resp_file")

if [[ -z "$JOB_ID" || "$JOB_ID" == "null" ]]; then
  echo "ERROR: gateway response missing job_id"
  cat "$gw_resp_file"
  exit 1
fi

if [[ "$queued_status" != "queued" ]]; then
  echo "ERROR: gateway response status is not queued"
  cat "$gw_resp_file"
  exit 1
fi

echo "OK: gateway queued job: $JOB_ID"

log_step "Verify job inserted in DB"
exists=$(psql_value "SELECT COUNT(*) FROM jobs WHERE id='${JOB_ID}';")
if [[ "$exists" != "1" ]]; then
  echo "ERROR: queued job not found in jobs table"
  exit 1
fi

attempts=$(psql_value "SELECT attempts FROM jobs WHERE id='${JOB_ID}';")
if [[ "$attempts" != "0" ]]; then
  echo "ERROR: expected attempts=0 immediately after enqueue, got $attempts"
  exit 1
fi

echo "OK: job persisted with attempts=0"

log_step "Verify worker pickup and completion"
start_epoch=$(date +%s)
picked="0"
completed="0"

while true; do
  status=$(psql_value "SELECT status FROM jobs WHERE id='${JOB_ID}';")
  locked_at=$(psql_value "SELECT COALESCE(to_char(locked_at, 'YYYY-MM-DD HH24:MI:SS'), '') FROM jobs WHERE id='${JOB_ID}';")

  if [[ "$status" == "running" || "$status" == "completed" ]]; then
    if [[ -n "$locked_at" ]]; then
      picked="1"
    fi
  fi

  if [[ "$status" == "completed" ]]; then
    completed="1"
    break
  fi

  now_epoch=$(date +%s)
  if (( now_epoch - start_epoch > POLL_TIMEOUT_SECONDS )); then
    break
  fi

  sleep "$POLL_INTERVAL_SECONDS"
done

if [[ "$picked" != "1" ]]; then
  echo "ERROR: worker pickup not observed (status never reached running/completed with locked_at)"
  exit 1
fi

if [[ "$completed" != "1" ]]; then
  final_status=$(psql_value "SELECT status FROM jobs WHERE id='${JOB_ID}';")
  final_attempts=$(psql_value "SELECT attempts FROM jobs WHERE id='${JOB_ID}';")
  echo "ERROR: job did not complete within timeout. status=$final_status attempts=$final_attempts"
  exit 1
fi

echo "OK: worker picked and completed job"

log_step "Optional runtime log assertion"
if [[ -n "$RUNTIME_LOG_CHECK_CMD" ]]; then
  if eval "$RUNTIME_LOG_CHECK_CMD" | grep -q "$FUNCTION_ID"; then
    echo "OK: runtime logs include function_id=$FUNCTION_ID"
  else
    echo "ERROR: runtime log check command did not include function_id=$FUNCTION_ID"
    exit 1
  fi
else
  echo "SKIP: set RUNTIME_LOG_CHECK_CMD to assert runtime logs deterministically"
fi

log_step "Final async wiring assertion"
echo "PASS: Gateway returned 202, job inserted, worker pickup observed, job completed."
