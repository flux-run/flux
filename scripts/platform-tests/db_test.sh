#!/usr/bin/env bash
# db_test.sh — comprehensive data access / cache / mutation tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0
QUERY_PAYLOAD='{"table":"users","operation":"select","limit":1}'

# ── Basic select ──────────────────────────────────────────────────────────────

s="$(gw_post "/db/query" "$QUERY_PAYLOAD" /tmp/t_db_q1.json /tmp/t_db_q1_hdr.txt)"
assert_status_and_jq "200" "$s" 'type == "object"' /tmp/t_db_q1.json "db query" FAIL

# ── Cache behaviour ───────────────────────────────────────────────────────────

s2="$(gw_post "/db/query" "$QUERY_PAYLOAD" /tmp/t_db_q2.json /tmp/t_db_q2_hdr.txt)"

cache1="$(grep -i '^x-cache:' /tmp/t_db_q1_hdr.txt | awk '{print toupper($2)}' | tr -d '\r' || true)"
cache2="$(grep -i '^x-cache:' /tmp/t_db_q2_hdr.txt | awk '{print toupper($2)}' | tr -d '\r' || true)"

if [[ "$s" == "200" && "$s2" == "200" && "$cache1" == "MISS" && "$cache2" == "HIT" ]]; then
  print_result 1 "db cache MISS then HIT"
else
  print_result 0 "db cache MISS then HIT" "x-cache1=${cache1:-none}, x-cache2=${cache2:-none}"
  FAIL=1
fi

# ── Malformed / invalid body → 400 ────────────────────────────────────────────

s="$(gw_post "/db/query" '{"table":' /tmp/t_db_bad.txt)"
assert_status "400" "$s" "db invalid JSON → 400" FAIL

# Missing required table field
s="$(gw_post "/db/query" '{"operation":"select","limit":1}' /tmp/t_db_notable.json)"
if [[ "$s" == "400" || "$s" == "422" ]]; then
  print_result 1 "db missing table → 400/422"
else
  print_result 0 "db missing table → 400/422" "HTTP ${s}"
  FAIL=1
fi

# Unsupported operation
s="$(gw_post "/db/query" '{"table":"users","operation":"drop"}' /tmp/t_db_drop.json)"
if [[ "$s" == "400" || "$s" == "403" || "$s" == "422" ]]; then
  print_result 1 "db unsupported operation → 4xx"
else
  print_result 0 "db unsupported operation → 4xx" "HTTP ${s}"
  FAIL=1
fi

# ── Pagination ────────────────────────────────────────────────────────────────

for lim in 1 5 10; do
  s="$(gw_post "/db/query" \
    "{\"table\":\"users\",\"operation\":\"select\",\"limit\":${lim}}" \
    /tmp/t_db_lim_${lim}.json)"
  if [[ "$s" == "200" ]]; then
    count="$(jq '(.data // .) | if type == "array" then length else (.rows // . | if type=="array" then length else 0 end) end' \
      /tmp/t_db_lim_${lim}.json 2>/dev/null || echo 999)"
    if [[ "$count" -le "$lim" ]]; then
      print_result 1 "db query limit=${lim}"
    else
      print_result 0 "db query limit=${lim}" "returned ${count} rows"
      FAIL=1
    fi
  else
    print_result 0 "db query limit=${lim}" "HTTP ${s}"
    FAIL=1
  fi
done

# ── offset / cursor pagination ────────────────────────────────────────────────

s1="$(gw_post "/db/query" \
  '{"table":"users","operation":"select","limit":2,"offset":0}' \
  /tmp/t_db_page0.json)"
s2="$(gw_post "/db/query" \
  '{"table":"users","operation":"select","limit":2,"offset":2}' \
  /tmp/t_db_page1.json)"
if [[ "$s1" == "200" && "$s2" == "200" ]]; then
  print_result 1 "db pagination offset 0 and 2"
else
  print_result 0 "db pagination offset" "HTTP s1=${s1} s2=${s2}"
  FAIL=1
fi

# ── Insert ────────────────────────────────────────────────────────────────────

SUFFIX="$(unique_suffix)"
INSERT_PAYLOAD="$(printf \
  '{"table":"users","operation":"insert","data":{"firebase_uid":"db-test-%s","email":"db-test-%s@example.com","name":"DB Test %s"}}' \
  "$SUFFIX" "$SUFFIX" "$SUFFIX")"

s="$(gw_post "/db/query" "$INSERT_PAYLOAD" /tmp/t_db_insert.json /tmp/t_db_insert_hdr.txt)"
if [[ "$s" == "200" || "$s" == "201" ]]; then
  print_result 1 "db insert"
  inserted_id="$(jq -r '(.data // .)[0].id // (.data // .).id // ""' /tmp/t_db_insert.json 2>/dev/null || true)"
else
  print_result 0 "db insert" "HTTP ${s}"
  inserted_id=""
  FAIL=1
fi

# ── Cache invalidation after write ───────────────────────────────────────────

if [[ -n "$inserted_id" ]]; then
  # First select on the new record (cold)
  sel_payload="{\"table\":\"users\",\"operation\":\"select\",\"filter\":{\"id\":\"${inserted_id}\"},\"limit\":1}"
  s_sel1="$(gw_post "/db/query" "$sel_payload" /tmp/t_db_sel_after_insert.json /tmp/t_db_sel_hdr.txt)"
  if [[ "$s_sel1" == "200" ]]; then
    cache_after_write="$(grep -i '^x-cache:' /tmp/t_db_sel_hdr.txt | awk '{print toupper($2)}' | tr -d '\r' || true)"
    if [[ "$cache_after_write" == "MISS" || -z "$cache_after_write" ]]; then
      print_result 1 "db cache MISS after insert"
    else
      print_result 0 "db cache MISS after insert" "x-cache=${cache_after_write} (should be MISS)"
      FAIL=1
    fi
  fi
fi

# ── Update ────────────────────────────────────────────────────────────────────

if [[ -n "$inserted_id" ]]; then
  UPDATE_PAYLOAD="{\"table\":\"users\",\"operation\":\"update\",\"filter\":{\"id\":\"${inserted_id}\"},\"data\":{\"name\":\"DB Updated ${SUFFIX}\"}}"
  s="$(gw_post "/db/query" "$UPDATE_PAYLOAD" /tmp/t_db_update.json)"
  if [[ "$s" == "200" ]]; then
    print_result 1 "db update"
  else
    print_result 0 "db update" "HTTP ${s}"
    FAIL=1
  fi
fi

# ── Delete ────────────────────────────────────────────────────────────────────

if [[ -n "$inserted_id" ]]; then
  DELETE_PAYLOAD="{\"table\":\"users\",\"operation\":\"delete\",\"filter\":{\"id\":\"${inserted_id}\"}}"
  s="$(gw_post "/db/query" "$DELETE_PAYLOAD" /tmp/t_db_delete.json)"
  if [[ "$s" == "200" || "$s" == "204" ]]; then
    print_result 1 "db delete"
  else
    print_result 0 "db delete" "HTTP ${s}"
    FAIL=1
  fi
fi

# ── Auth required ─────────────────────────────────────────────────────────────

s="$(curl -sS -o /dev/null -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/db/query" \
  -H "Content-Type: application/json" \
  --data '{"table":"users","operation":"select","limit":1}' || true)"
assert_status "401" "$s" "db requires auth" FAIL

exit "$FAIL"
