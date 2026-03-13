#!/usr/bin/env bash
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0
MUTATION_DATABASE="${MUTATION_DATABASE:-main}"
MUTATION_TABLE="${MUTATION_TABLE:-users}"
MUTATION_PK_FIELD="${MUTATION_PK_FIELD:-id}"

if [[ -n "${MUTATION_INSERT_JSON:-}" ]]; then
  INSERT_QUERY="${MUTATION_INSERT_JSON}"
else
  unique="$(unique_suffix)"
  INSERT_QUERY="$(printf '{"database":"%s","table":"%s","operation":"insert","data":{"firebase_uid":"platform-%s","email":"platform-%s@example.com","name":"Platform %s"}}' \
    "$MUTATION_DATABASE" "$MUTATION_TABLE" "$unique" "$unique" "$unique")"
fi

started_at="$(now_rfc3339)"

status="$(curl -sS -D /tmp/platform_mutation_headers.txt -o /tmp/platform_mutation_body.json -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/db/query" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data "${INSERT_QUERY}")"

finished_at="$(now_rfc3339)"
request_id="$(http_header_value /tmp/platform_mutation_headers.txt x-request-id || true)"
record_id="$(jq -er --arg field "$MUTATION_PK_FIELD" '.data[0][$field] // .data[$field]' /tmp/platform_mutation_body.json 2>/dev/null || true)"

if [[ "$status" == "200" && -n "$request_id" && -n "$record_id" ]]; then
  print_result 1 "state write request"
else
  print_result 0 "state write request" "status=${status}, request_id=${request_id:-missing}, record_id=${record_id:-missing}"
  exit 1
fi

export REQUEST_ID_EXPECTED="$request_id"
export MUTATION_TABLE_EXPECTED="$MUTATION_TABLE"
export RECORD_ID_EXPECTED="$record_id"

mutations_url="${API_URL}/db/mutations?request_id=${request_id}&limit=50"
if wait_for_json_match \
  "mutation log" \
  "$mutations_url" \
  '.request_id == env.REQUEST_ID_EXPECTED
   and (.count >= 1)
   and any(.mutations[];
     .table_name == env.MUTATION_TABLE_EXPECTED
     and .operation == "insert"
     and (.after_state != null)
   )' \
  /tmp/platform_mutations.json 20 1; then
  print_result 1 "mutation log"
else
  print_result 0 "mutation log" "request_id=${request_id}"
  FAIL=1
fi

history_query="${MUTATION_HISTORY_QUERY:-}"
if [[ -z "$history_query" ]]; then
  if [[ "$MUTATION_PK_FIELD" == "id" ]]; then
    history_query="id=${record_id}"
  else
    history_pk_json="$(jq -nc --arg field "$MUTATION_PK_FIELD" --arg value "$record_id" '{($field): $value}')"
    history_query="pk=$(jq -rn --arg v "$history_pk_json" '$v|@uri')"
  fi
fi

history_url="${API_URL}/db/history/${MUTATION_DATABASE}/${MUTATION_TABLE}?${history_query}&limit=20"
if wait_for_json_match \
  "state history" \
  "$history_url" \
  '.table == env.MUTATION_TABLE_EXPECTED
   and (.count >= 1)
   and any(.history[];
     .operation == "insert"
     and ((.after_state.id // "") == env.RECORD_ID_EXPECTED or env.RECORD_ID_EXPECTED == "")
   )' \
  /tmp/platform_history.json 20 1; then
  print_result 1 "state history"
else
  print_result 0 "state history" "table=${MUTATION_TABLE}, record_id=${record_id}"
  FAIL=1
fi

from_ts="$(jq -rn --arg v "$started_at" '$v|@uri')"
to_ts="$(jq -rn --arg v "$finished_at" '$v|@uri')"
replay_url="${API_URL}/db/replay/${MUTATION_DATABASE}?from=${from_ts}&to=${to_ts}&limit=200"

if wait_for_json_match \
  "replay window" \
  "$replay_url" \
  '(.count >= 1)
   and any(.replay[];
     .table_name == env.MUTATION_TABLE_EXPECTED
     and ((.request_id // "") == env.REQUEST_ID_EXPECTED)
   )' \
  /tmp/platform_replay.json 20 1; then
  print_result 1 "replay window"
else
  print_result 0 "replay window" "request_id=${request_id}"
  FAIL=1
fi

exit "$FAIL"
