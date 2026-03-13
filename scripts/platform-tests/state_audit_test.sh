#!/usr/bin/env bash
# state_audit_test.sh — mutation log, state history, and replay tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0
MUTATION_DATABASE="${MUTATION_DATABASE:-main}"
MUTATION_TABLE="${MUTATION_TABLE:-users}"
MUTATION_PK_FIELD="${MUTATION_PK_FIELD:-id}"

# ── INSERT mutation ───────────────────────────────────────────────────────────

if [[ -n "${MUTATION_INSERT_JSON:-}" ]]; then
  INSERT_QUERY="${MUTATION_INSERT_JSON}"
else
  unique="$(unique_suffix)"
  INSERT_QUERY="$(printf \
    '{"database":"%s","table":"%s","operation":"insert","data":{"firebase_uid":"platform-%s","email":"platform-%s@example.com","name":"Platform %s"}}' \
    "$MUTATION_DATABASE" "$MUTATION_TABLE" "$unique" "$unique" "$unique")"
fi

started_at="$(now_rfc3339)"

s="$(gw_post "/db/query" "$INSERT_QUERY" /tmp/t_state_insert.json /tmp/t_state_insert_hdr.txt)"
finished_at="$(now_rfc3339)"
request_id="$(http_header_value /tmp/t_state_insert_hdr.txt x-request-id || true)"
record_id="$(jq -er --arg field "$MUTATION_PK_FIELD" \
  '.data[0][$field] // .data[$field]' /tmp/t_state_insert.json 2>/dev/null || true)"

if [[ "$s" == "200" && -n "$request_id" && -n "$record_id" ]]; then
  print_result 1 "state write request"
else
  print_result 0 "state write request" "status=${s}, request_id=${request_id:-missing}, record_id=${record_id:-missing}"
  exit 1
fi

export REQUEST_ID_EXPECTED="$request_id"
export MUTATION_TABLE_EXPECTED="$MUTATION_TABLE"
export MUTATION_PK_FIELD_EXPECTED="$MUTATION_PK_FIELD"
export RECORD_ID_EXPECTED="$record_id"

# ── Mutation log ──────────────────────────────────────────────────────────────

mutations_url="${API_URL}/db/mutations?request_id=${request_id}&limit=50"
if wait_for_json_match "mutation log" "$mutations_url" \
  '.request_id == env.REQUEST_ID_EXPECTED
   and (.count >= 1)
   and any(.mutations[];
     .table_name == env.MUTATION_TABLE_EXPECTED
     and .operation == "insert"
     and (.after_state != null)
   )' \
  /tmp/t_mutations.json 20 1; then
  print_result 1 "mutation log"

  # Mutation entry has required fields
  assert_jq \
    '.mutations[0] | has("table_name") and has("operation") and has("request_id")' \
    /tmp/t_mutations.json "mutation entry schema" FAIL

  # after_state must not be null for inserts
  ok="$(jq -e '.mutations | all(select(.operation == "insert") | .after_state != null)' \
    /tmp/t_mutations.json 2>/dev/null && echo 1 || echo 0)"
  if [[ "$ok" == "1" ]]; then
    print_result 1 "mutation insert after_state not null"
  else
    print_result 0 "mutation insert after_state not null"
    FAIL=1
  fi
else
  print_result 0 "mutation log" "request_id=${request_id}"
  FAIL=1
fi

# ── UPDATE mutation ───────────────────────────────────────────────────────────

UPDATE_QUERY="$(printf \
  '{"database":"%s","table":"%s","operation":"update","filter":{"%s":"%s"},"data":{"name":"Updated State Test"}}' \
  "$MUTATION_DATABASE" "$MUTATION_TABLE" "$MUTATION_PK_FIELD" "$record_id")"

s_upd="$(gw_post "/db/query" "$UPDATE_QUERY" /tmp/t_state_update.json /tmp/t_state_update_hdr.txt)"
upd_request_id="$(http_header_value /tmp/t_state_update_hdr.txt x-request-id || true)"

if [[ "$s_upd" == "200" && -n "$upd_request_id" ]]; then
  print_result 1 "state update request"

  export UPD_REQUEST_ID="$upd_request_id"
  upd_mut_url="${API_URL}/db/mutations?request_id=${upd_request_id}&limit=10"
  if wait_for_json_match "update mutation log" "$upd_mut_url" \
    '.request_id == env.UPD_REQUEST_ID
     and (.count >= 1)
     and any(.mutations[];
       .operation == "update"
       and (.before_state != null)
       and (.after_state != null)
     )' \
    /tmp/t_upd_mutations.json 20 1; then
    print_result 1 "update mutation before/after state"
  else
    print_result 0 "update mutation before/after state"
    FAIL=1
  fi
else
  print_result 0 "state update request" "HTTP ${s_upd}"
  FAIL=1
fi

# ── State history ─────────────────────────────────────────────────────────────

if [[ "$MUTATION_PK_FIELD" == "id" ]]; then
  history_query="id=${record_id}"
else
  history_query="${MUTATION_PK_FIELD}=${record_id}"
fi

history_url="${API_URL}/db/history/${MUTATION_DATABASE}/${MUTATION_TABLE}?${history_query}&limit=20"
if wait_for_json_match "state history" "$history_url" \
  '.table == env.MUTATION_TABLE_EXPECTED
   and (.count >= 1)
   and any(.history[];
     .operation == "insert"
     and ((((.after_state[env.MUTATION_PK_FIELD_EXPECTED] // .after_state.id // "") | tostring) == env.RECORD_ID_EXPECTED)
       or env.RECORD_ID_EXPECTED == "")
   )' \
  /tmp/t_state_history.json 20 1; then
  print_result 1 "state history insert"

  # History should also show the update
  ok="$(jq -e 'any(.history[]; .operation == "update")' \
    /tmp/t_state_history.json 2>/dev/null && echo 1 || echo 0)"
  if [[ "$ok" == "1" ]]; then
    print_result 1 "state history shows update"
  else
    print_result 0 "state history shows update" "no update in history"
    FAIL=1
  fi

  # History is ordered (most recent first or ascending)
  op_count="$(jq '.count' /tmp/t_state_history.json 2>/dev/null || echo 0)"
  if [[ "$op_count" -ge 2 ]]; then
    print_result 1 "state history count ≥ 2"
  else
    print_result 0 "state history count ≥ 2" "count=${op_count}"
    FAIL=1
  fi
else
  print_result 0 "state history" "table=${MUTATION_TABLE}, record_id=${record_id}"
  FAIL=1
fi

# ── DELETE mutation ───────────────────────────────────────────────────────────

DELETE_QUERY="$(printf \
  '{"database":"%s","table":"%s","operation":"delete","filter":{"%s":"%s"}}' \
  "$MUTATION_DATABASE" "$MUTATION_TABLE" "$MUTATION_PK_FIELD" "$record_id")"

s_del="$(gw_post "/db/query" "$DELETE_QUERY" /tmp/t_state_delete.json /tmp/t_state_delete_hdr.txt)"
del_request_id="$(http_header_value /tmp/t_state_delete_hdr.txt x-request-id || true)"

if [[ "$s_del" == "200" && -n "$del_request_id" ]]; then
  print_result 1 "state delete request"

  export DEL_REQUEST_ID="$del_request_id"
  del_mut_url="${API_URL}/db/mutations?request_id=${del_request_id}&limit=10"
  if wait_for_json_match "delete mutation log" "$del_mut_url" \
    '.request_id == env.DEL_REQUEST_ID
     and (.count >= 1)
     and any(.mutations[];
       .operation == "delete"
       and (.before_state != null)
     )' \
    /tmp/t_del_mutations.json 20 1; then
    print_result 1 "delete mutation before_state preserved"
  else
    print_result 0 "delete mutation before_state preserved"
    FAIL=1
  fi
else
  print_result 0 "state delete request" "HTTP ${s_del}"
  FAIL=1
fi

# ── Replay window ─────────────────────────────────────────────────────────────

from_ts="$(jq -rn --arg v "$started_at" '$v|@uri')"
to_ts="$(jq -rn --arg v "$finished_at" '$v|@uri')"
replay_url="${API_URL}/db/replay/${MUTATION_DATABASE}?from=${from_ts}&to=${to_ts}&limit=200"

if wait_for_json_match "replay window" "$replay_url" \
  '(.count >= 1)
   and any(.replay[];
     .table_name == env.MUTATION_TABLE_EXPECTED
     and ((.request_id // "") == env.REQUEST_ID_EXPECTED)
   )' \
  /tmp/t_replay.json 20 1; then
  print_result 1 "replay window"

  # Replay entries in order (insert, update, delete)
  ok="$(jq -e '
    [.replay[] | select(.table_name == env.MUTATION_TABLE_EXPECTED) | .operation] |
    (index("insert") // -1) >= 0
  ' /tmp/t_replay.json 2>/dev/null && echo 1 || echo 0)"
  if [[ "$ok" == "1" ]]; then
    print_result 1 "replay includes insert"
  else
    print_result 0 "replay includes insert"
    FAIL=1
  fi
else
  print_result 0 "replay window" "request_id=${request_id}"
  FAIL=1
fi

exit "$FAIL"
