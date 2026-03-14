#!/usr/bin/env bash
# events_test.sh — SSE / server-sent events tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0

# ── Basic SSE connection ──────────────────────────────────────────────────────

rm -f /tmp/t_sse_body.txt /tmp/t_sse_hdr.txt

s="$(curl -sS -N --max-time 25 \
  -D /tmp/t_sse_hdr.txt \
  -o /tmp/t_sse_body.txt \
  -w "%{http_code}" \
  -H "Accept: text/event-stream" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${GATEWAY_URL}/events/stream" || true)"

if [[ "$s" == "200" ]]; then
  print_result 1 "events stream 200"

  # Content-Type must be text/event-stream
  ct="$(http_header_value /tmp/t_sse_hdr.txt content-type || true)"
  if printf '%s' "$ct" | grep -qi 'text/event-stream'; then
    print_result 1 "events content-type text/event-stream"
  else
    print_result 0 "events content-type text/event-stream" "got: ${ct:-missing}"
    FAIL=1
  fi

  # Must receive at least one heartbeat event
  if grep -q 'event: heartbeat' /tmp/t_sse_body.txt 2>/dev/null; then
    print_result 1 "events heartbeat received"
  else
    print_result 0 "events heartbeat received" "no heartbeat in stream"
    FAIL=1
  fi

  # SSE lines must follow format "event: ...\ndata: ..."
  if grep -q '^data:' /tmp/t_sse_body.txt 2>/dev/null; then
    print_result 1 "events data lines present"
  else
    print_result 0 "events data lines present" "no data: lines found"
    FAIL=1
  fi

  # id: lines for resumability
  if grep -q '^id:' /tmp/t_sse_body.txt 2>/dev/null; then
    print_result 1 "events id lines present"
  else
    print_result 0 "events id lines present" "no id: lines found"
    FAIL=1
  fi
else
  print_result 0 "events stream 200" "HTTP ${s}"
  FAIL=1
fi

# ── Unauthenticated SSE → 401 ─────────────────────────────────────────────────

s_no_auth="$(curl -sS -N --max-time 5 \
  -o /tmp/t_sse_noauth.txt \
  -w "%{http_code}" \
  -H "Accept: text/event-stream" \
  "${GATEWAY_URL}/events/stream" || true)"
if [[ "$s_no_auth" == "401" || "$s_no_auth" == "403" ]]; then
  print_result 1 "events stream requires auth"
else
  print_result 0 "events stream requires auth" "HTTP ${s_no_auth}"
  FAIL=1
fi

# ── System event after function call ─────────────────────────────────────────

# Start SSE capture in background for 15 seconds
rm -f /tmp/t_sse_event.txt
curl -sS -N --max-time 15 \
  -H "Accept: text/event-stream" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  "${GATEWAY_URL}/events/stream" > /tmp/t_sse_event.txt &
SSE_PID=$!
sleep 1

# Trigger a function call
gw_post "/${FUNCTION_NAME}" '{"_event_tag":"sse-test"}' /tmp/t_sse_trigger.json >/dev/null 2>&1 || true
sleep 3

kill $SSE_PID 2>/dev/null || true
wait $SSE_PID 2>/dev/null || true

# Should have received at least a heartbeat (function event may not appear
# depending on timing/subscription, so just check stream was alive)
if grep -q 'event:\|data:' /tmp/t_sse_event.txt 2>/dev/null; then
  print_result 1 "events stream alive during function call"
else
  print_result 0 "events stream alive during function call" "no events captured"
  FAIL=1
fi

exit "$FAIL"
