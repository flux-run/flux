#!/usr/bin/env bash
# data_engine_test.sh — comprehensive data engine tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init
require_env DB_URL

FAIL=0
token="$(data_engine_service_token)"
SUFFIX="$(unique_suffix)"

# ── Health ────────────────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_de_health.json -w "%{http_code}" "${DB_URL}/health" || true)"
assert_status_and_jq "200" "$s" '.status == "ok"' /tmp/t_de_health.json "data-engine health" FAIL

# ── Auth ──────────────────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_de_unauth.json -w "%{http_code}" \
  -X POST "${DB_URL}/db/query" \
  -H "Content-Type: application/json" \
  --data '{"database":"main","table":"users","operation":"select","limit":1}' || true)"
assert_status "401" "$s" "data-engine auth required" FAIL

# ── Basic select ──────────────────────────────────────────────────────────────

req_id="de-sel-${SUFFIX}"
s="$(curl -sS -o /tmp/t_de_query.json -w "%{http_code}" \
  -X POST "${DB_URL}/db/query" \
  -H "Content-Type: application/json" \
  -H "x-service-token: ${token}" \
  -H "x-request-id: ${req_id}" \
  -H "x-user-role: service" \
  --data '{"database":"main","table":"users","operation":"select","limit":1}' || true)"

assert_status_and_jq "200" "$s" \
  --arg rid "$req_id" '.meta.request_id == $rid and has("data") and has("meta")' \
  /tmp/t_de_query.json "data-engine select" FAIL

# Response schema check
assert_jq 'has("data") and (.meta | has("request_id") and has("duration_ms"))' \
  /tmp/t_de_query.json "data-engine response schema" FAIL

# ── Select with limit ─────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_de_lim.json -w "%{http_code}" \
  -X POST "${DB_URL}/db/query" \
  -H "Content-Type: application/json" \
  -H "x-service-token: ${token}" \
  -H "x-request-id: de-lim-${SUFFIX}" \
  -H "x-user-role: service" \
  --data '{"database":"main","table":"users","operation":"select","limit":2}' || true)"
if [[ "$s" == "200" ]]; then
  count="$(jq '.data | if type=="array" then length else 0 end' /tmp/t_de_lim.json 2>/dev/null || echo 99)"
  if [[ "$count" -le 2 ]]; then
    print_result 1 "data-engine select limit=2"
  else
    print_result 0 "data-engine select limit=2" "returned ${count} rows"
    FAIL=1
  fi
else
  print_result 0 "data-engine select limit=2" "HTTP ${s}"
  FAIL=1
fi

# ── Insert ────────────────────────────────────────────────────────────────────

insert_req_id="de-ins-${SUFFIX}"
s="$(curl -sS -o /tmp/t_de_insert.json -w "%{http_code}" \
  -X POST "${DB_URL}/db/query" \
  -H "Content-Type: application/json" \
  -H "x-service-token: ${token}" \
  -H "x-request-id: ${insert_req_id}" \
  -H "x-user-role: service" \
  --data "{\"database\":\"main\",\"table\":\"users\",\"operation\":\"insert\",\"data\":{\"firebase_uid\":\"de-test-${SUFFIX}\",\"email\":\"de-${SUFFIX}@example.com\",\"name\":\"DE Test\"}}" || true)"

if [[ "$s" == "200" || "$s" == "201" ]]; then
  print_result 1 "data-engine insert"
  inserted_id="$(jq -r '(.data // .)[0].id // (.data // .).id // ""' /tmp/t_de_insert.json 2>/dev/null || true)"
else
  print_result 0 "data-engine insert" "HTTP ${s}"
  inserted_id=""
  FAIL=1
fi

# ── Update ────────────────────────────────────────────────────────────────────

if [[ -n "$inserted_id" ]]; then
  s="$(curl -sS -o /tmp/t_de_update.json -w "%{http_code}" \
    -X POST "${DB_URL}/db/query" \
    -H "Content-Type: application/json" \
    -H "x-service-token: ${token}" \
    -H "x-request-id: de-upd-${SUFFIX}" \
    -H "x-user-role: service" \
    --data "{\"database\":\"main\",\"table\":\"users\",\"operation\":\"update\",\"filter\":{\"id\":\"${inserted_id}\"},\"data\":{\"name\":\"DE Updated\"}}" || true)"
  if [[ "$s" == "200" ]]; then
    print_result 1 "data-engine update"
  else
    print_result 0 "data-engine update" "HTTP ${s}"
    FAIL=1
  fi
fi

# ── Select with filter ────────────────────────────────────────────────────────

if [[ -n "$inserted_id" ]]; then
  s="$(curl -sS -o /tmp/t_de_filter.json -w "%{http_code}" \
    -X POST "${DB_URL}/db/query" \
    -H "Content-Type: application/json" \
    -H "x-service-token: ${token}" \
    -H "x-request-id: de-flt-${SUFFIX}" \
    -H "x-user-role: service" \
    --data "{\"database\":\"main\",\"table\":\"users\",\"operation\":\"select\",\"filter\":{\"id\":\"${inserted_id}\"},\"limit\":1}" || true)"
  if [[ "$s" == "200" ]]; then
    ok="$(jq -e --arg id "$inserted_id" '.data | any(.[]; .id == $id)' \
      /tmp/t_de_filter.json 2>/dev/null && echo 1 || echo 0)"
    if [[ "$ok" == "1" ]]; then
      print_result 1 "data-engine select by filter"
    else
      print_result 0 "data-engine select by filter" "record not found"
      FAIL=1
    fi
  else
    print_result 0 "data-engine select by filter" "HTTP ${s}"
    FAIL=1
  fi
fi

# ── Delete ────────────────────────────────────────────────────────────────────

if [[ -n "$inserted_id" ]]; then
  s="$(curl -sS -o /tmp/t_de_delete.json -w "%{http_code}" \
    -X POST "${DB_URL}/db/query" \
    -H "Content-Type: application/json" \
    -H "x-service-token: ${token}" \
    -H "x-request-id: de-del-${SUFFIX}" \
    -H "x-user-role: service" \
    --data "{\"database\":\"main\",\"table\":\"users\",\"operation\":\"delete\",\"filter\":{\"id\":\"${inserted_id}\"}}" || true)"
  if [[ "$s" == "200" || "$s" == "204" ]]; then
    print_result 1 "data-engine delete"
  else
    print_result 0 "data-engine delete" "HTTP ${s}"
    FAIL=1
  fi
fi

# ── Duration in response ──────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_de_dur.json -w "%{http_code}" \
  -X POST "${DB_URL}/db/query" \
  -H "Content-Type: application/json" \
  -H "x-service-token: ${token}" \
  -H "x-request-id: de-dur-${SUFFIX}" \
  -H "x-user-role: service" \
  --data '{"database":"main","table":"users","operation":"select","limit":1}' || true)"
if [[ "$s" == "200" ]]; then
  dur="$(jq '.meta.duration_ms // -1' /tmp/t_de_dur.json 2>/dev/null || echo -1)"
  if python3 -c "import sys; sys.exit(0 if float('${dur}') >= 0 else 1)" 2>/dev/null; then
    print_result 1 "data-engine meta.duration_ms (${dur}ms)"
  else
    print_result 0 "data-engine meta.duration_ms" "got: ${dur}"
    FAIL=1
  fi
fi

# ── Malformed body → 400 ──────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_de_bad.json -w "%{http_code}" \
  -X POST "${DB_URL}/db/query" \
  -H "Content-Type: application/json" \
  -H "x-service-token: ${token}" \
  --data '{bad' || true)"
if [[ "$s" == "400" || "$s" == "422" ]]; then
  print_result 1 "data-engine malformed body → 400/422"
else
  print_result 0 "data-engine malformed body → 400/422" "HTTP ${s}"
  FAIL=1
fi

exit "$FAIL"
