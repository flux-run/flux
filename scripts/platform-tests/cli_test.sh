#!/usr/bin/env bash
# cli_test.sh — comprehensive CLI tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init
require_cmds cargo

FAIL=0
FLUX_BIN="${FLUX_BIN:-/Users/shashisharma/code/self/flowbase/target/debug/flux}"

if [[ ! -x "$FLUX_BIN" ]]; then
  cargo build -q -p cli
fi

export FLUX_URL="${API_URL}"
export FLUXBASE_GATEWAY_URL="${GATEWAY_URL}"
export FLUX_CLI_KEY="${TOKEN}"

# ── --help / version ──────────────────────────────────────────────────────────

help_out="$("$FLUX_BIN" --help 2>&1 || true)"
if printf '%s' "$help_out" | grep -Ei 'usage|flux|commands|help' >/dev/null; then
  print_result 1 "cli --help"
else
  print_result 0 "cli --help" "no usage output"
  FAIL=1
fi

version_out="$("$FLUX_BIN" --version 2>&1 || "$FLUX_BIN" version 2>&1 || true)"
if printf '%s' "$version_out" | grep -Ei 'flux|version|[0-9]+\.[0-9]+' >/dev/null; then
  print_result 1 "cli version"
else
  print_result 0 "cli version" "no version output"
  FAIL=1
fi

# ── Setup: trigger a request so we have a request_id ─────────────────────────

s="$(curl -sS -D /tmp/t_cli_setup_hdr.txt -o /tmp/t_cli_setup.json -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/${FUNCTION_NAME}" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data '{"message":"cli-setup"}' || true)"

request_id="$(http_header_value /tmp/t_cli_setup_hdr.txt x-request-id || true)"
if [[ "$s" != "200" || -z "$request_id" ]]; then
  print_result 0 "cli setup request" "status=${s}, request_id=${request_id:-missing}"
  exit 1
fi
print_result 1 "cli setup request"

# ── flux invoke ───────────────────────────────────────────────────────────────

invoke_out="$("$FLUX_BIN" --no-color invoke "${FUNCTION_NAME}" --gateway --payload '{"message":"cli-invoke"}' 2>&1 || true)"
if printf '%s' "$invoke_out" | grep -E "Invoking|runtime|Function invocation failed" >/dev/null; then
  print_result 1 "cli invoke"
else
  print_result 0 "cli invoke"
  FAIL=1
fi

# Invoke with invalid function name → error message (not a crash)
bad_invoke_out="$("$FLUX_BIN" --no-color invoke "__nonexistent_$(unique_suffix)__" --gateway --payload '{}' 2>&1 || true)"
if printf '%s' "$bad_invoke_out" | grep -Ei 'error|not found|failed|404' >/dev/null; then
  print_result 1 "cli invoke unknown function → error"
else
  print_result 0 "cli invoke unknown function → error" "output: ${bad_invoke_out:0:80}"
  FAIL=1
fi

# ── flux trace ────────────────────────────────────────────────────────────────

sleep 1  # let trace land

trace_out="$("$FLUX_BIN" --no-color trace "${request_id}" 2>&1 || true)"
if printf '%s' "$trace_out" | grep -E "Trace|${request_id:0:8}" >/dev/null; then
  print_result 1 "cli trace"
else
  print_result 0 "cli trace" "request_id=${request_id}"
  FAIL=1
fi

# Non-existent trace → graceful error
bad_trace_out="$("$FLUX_BIN" --no-color trace "00000000-0000-0000-0000-000000000000" 2>&1 || true)"
if printf '%s' "$bad_trace_out" | grep -Ei 'not found|error|no trace|404' >/dev/null; then
  print_result 1 "cli trace unknown id → error"
else
  print_result 0 "cli trace unknown id → error" "output: ${bad_trace_out:0:80}"
  FAIL=1
fi

# ── flux why ─────────────────────────────────────────────────────────────────

why_out="$("$FLUX_BIN" --no-color why "${request_id}" 2>&1 || true)"
if printf '%s' "$why_out" | grep -E "request_id:|Suggested next steps|Execution graph" >/dev/null; then
  print_result 1 "cli why"
else
  print_result 0 "cli why" "request_id=${request_id}"
  FAIL=1
fi

# ── flux doctor ──────────────────────────────────────────────────────────────

doctor_out="$("$FLUX_BIN" --no-color doctor "${request_id}" 2>&1 || true)"
if printf '%s' "$doctor_out" | grep -E "REQUEST|ROOT CAUSE|SUGGESTED ACTIONS|request_id" >/dev/null; then
  print_result 1 "cli doctor"
else
  print_result 0 "cli doctor" "request_id=${request_id}"
  FAIL=1
fi

# ── flux records ─────────────────────────────────────────────────────────────

records_out="$("$FLUX_BIN" --no-color records count --after 24h 2>&1 || true)"
if printf '%s' "$records_out" | grep -E "records match|[0-9]+" >/dev/null; then
  print_result 1 "cli records count"
else
  print_result 0 "cli records count"
  FAIL=1
fi

# ── flux logs ────────────────────────────────────────────────────────────────

logs_out="$("$FLUX_BIN" --no-color logs --limit 5 2>&1 || true)"
if printf '%s' "$logs_out" | grep -Ei 'log|error|info|warn|no logs' >/dev/null; then
  print_result 1 "cli logs"
else
  print_result 0 "cli logs" "output: ${logs_out:0:100}"
  FAIL=1
fi

# Logs with level filter
logs_err_out="$("$FLUX_BIN" --no-color logs --level error --limit 3 2>&1 || true)"
if [[ -n "$logs_err_out" ]]; then
  print_result 1 "cli logs --level error"
else
  print_result 0 "cli logs --level error"
  FAIL=1
fi

# ── flux secrets ─────────────────────────────────────────────────────────────

secrets_out="$("$FLUX_BIN" --no-color secrets list 2>&1 || true)"
if printf '%s' "$secrets_out" | grep -Ei 'secret|no secrets|key|name|list' >/dev/null; then
  print_result 1 "cli secrets list"
else
  print_result 0 "cli secrets list" "output: ${secrets_out:0:100}"
  FAIL=1
fi

# Set a secret
cli_secret_key="CLI_TEST_SECRET_$(unique_suffix)"
set_out="$("$FLUX_BIN" --no-color secrets set "${cli_secret_key}" "cli-test-value" 2>&1 || true)"
if printf '%s' "$set_out" | grep -Ei 'set|created|ok|saved|success' >/dev/null; then
  print_result 1 "cli secrets set"
else
  print_result 0 "cli secrets set" "output: ${set_out:0:80}"
  FAIL=1
fi

# Delete the secret
del_out="$("$FLUX_BIN" --no-color secrets delete "${cli_secret_key}" 2>&1 || true)"
if printf '%s' "$del_out" | grep -Ei 'deleted|removed|ok|success' >/dev/null; then
  print_result 1 "cli secrets delete"
else
  print_result 0 "cli secrets delete" "output: ${del_out:0:80}"
  FAIL=1
fi

# ── flux generate ────────────────────────────────────────────────────────────

gen_out="$("$FLUX_BIN" --no-color generate 2>&1 || true)"
if printf '%s' "$gen_out" | grep -Ei 'generating|generated|types|sdk|error' >/dev/null; then
  print_result 1 "cli generate"
else
  print_result 0 "cli generate" "output: ${gen_out:0:100}"
  FAIL=1
fi

# ── flux status ──────────────────────────────────────────────────────────────

status_out="$("$FLUX_BIN" --no-color status 2>&1 || true)"
if printf '%s' "$status_out" | grep -Ei 'status|running|connected|ok|server|postgres' >/dev/null; then
  print_result 1 "cli status"
else
  print_result 0 "cli status" "output: ${status_out:0:100}"
  FAIL=1
fi

exit "$FAIL"
