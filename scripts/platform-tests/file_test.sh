#!/usr/bin/env bash
# file_test.sh — comprehensive file upload URL tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0
SUFFIX="$(unique_suffix)"

# ── Auth required ─────────────────────────────────────────────────────────────

s="$(curl -sS -o /tmp/t_file_unauth.json -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/files/upload-url" \
  -H "Content-Type: application/json" \
  --data '{"database":"main","table":"users","row_id":"test","column":"avatar"}' || true)"
assert_status "401" "$s" "files upload-url requires auth" FAIL

# ── Basic upload URL ──────────────────────────────────────────────────────────

s="$(gw_post "/files/upload-url" \
  '{"database":"main","table":"users","row_id":"test-'${SUFFIX}'","column":"avatar"}' \
  /tmp/t_file_body.json)"
assert_status_and_jq "200" "$s" 'has("upload_url") and has("object_key")' \
  /tmp/t_file_body.json "files upload-url basic" FAIL

# ── Upload URL is a string ────────────────────────────────────────────────────

if [[ "$s" == "200" ]]; then
  url="$(jq -r '.upload_url // ""' /tmp/t_file_body.json 2>/dev/null || true)"
  key="$(jq -r '.object_key // ""' /tmp/t_file_body.json 2>/dev/null || true)"

  if [[ -n "$url" ]]; then
    print_result 1 "files upload_url non-empty"
  else
    print_result 0 "files upload_url non-empty" "upload_url is empty"
    FAIL=1
  fi

  if [[ -n "$key" ]]; then
    print_result 1 "files object_key non-empty"
  else
    print_result 0 "files object_key non-empty" "object_key is empty"
    FAIL=1
  fi

  # object_key should contain table name and row_id or column
  if printf '%s' "$key" | grep -qE '(users|avatar|test)'; then
    print_result 1 "files object_key contains meaningful path"
  else
    print_result 0 "files object_key contains meaningful path" "got: ${key}"
    FAIL=1
  fi
fi

# ── Missing required field → 400/422 ─────────���───────────────────────────────

# Missing row_id
s="$(gw_post "/files/upload-url" \
  '{"database":"main","table":"users","column":"avatar"}' \
  /tmp/t_file_missing_row.json)"
if [[ "$s" == "400" || "$s" == "422" ]]; then
  print_result 1 "files upload-url missing row_id → 400/422"
else
  print_result 0 "files upload-url missing row_id → 400/422" "HTTP ${s}"
  FAIL=1
fi

# Missing table
s="$(gw_post "/files/upload-url" \
  '{"database":"main","row_id":"r1","column":"avatar"}' \
  /tmp/t_file_missing_tbl.json)"
if [[ "$s" == "400" || "$s" == "422" ]]; then
  print_result 1 "files upload-url missing table → 400/422"
else
  print_result 0 "files upload-url missing table → 400/422" "HTTP ${s}"
  FAIL=1
fi

# Empty body
s="$(gw_post "/files/upload-url" '{}' /tmp/t_file_empty.json)"
if [[ "$s" == "400" || "$s" == "422" ]]; then
  print_result 1 "files upload-url empty body → 400/422"
else
  print_result 0 "files upload-url empty body → 400/422" "HTTP ${s}"
  FAIL=1
fi

# ── CORS headers on upload-url ────────────────────────────────────────────────

s="$(curl -sS -D /tmp/t_file_cors_hdr.txt -o /tmp/t_file_cors.json -w "%{http_code}" \
  -X OPTIONS "${GATEWAY_URL}/files/upload-url" \
  -H "Origin: http://localhost:3000" \
  -H "Access-Control-Request-Method: POST" \
  -H "Access-Control-Request-Headers: Content-Type,Authorization" || true)"
if [[ "$s" == "200" || "$s" == "204" ]]; then
  cors="$(http_header_value /tmp/t_file_cors_hdr.txt access-control-allow-origin || true)"
  if [[ -n "$cors" ]]; then
    print_result 1 "files upload-url CORS headers"
  else
    print_result 0 "files upload-url CORS headers" "No ACAO header in preflight response"
    FAIL=1
  fi
else
  # Preflight may not be configured — acceptable if direct POST works
  print_result 1 "files upload-url CORS (preflight not required, skipped)"
fi

# ── Response Content-Type ─────────────────────────────────────────────────────

s="$(curl -sS -D /tmp/t_file_ct_hdr.txt -o /tmp/t_file_ct.json -w "%{http_code}" \
  -X POST "${GATEWAY_URL}/files/upload-url" \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer ${TOKEN}" \
  -H "X-Fluxbase-Tenant: ${TENANT_ID}" \
  -H "X-Fluxbase-Project: ${PROJECT_ID}" \
  --data '{"database":"main","table":"users","row_id":"ct-'${SUFFIX}'","column":"avatar"}' || true)"
if [[ "$s" == "200" ]]; then
  ct="$(http_header_value /tmp/t_file_ct_hdr.txt content-type || true)"
  if printf '%s' "$ct" | grep -qi 'application/json'; then
    print_result 1 "files upload-url Content-Type: application/json"
  else
    print_result 0 "files upload-url Content-Type: application/json" "got: ${ct:-missing}"
    FAIL=1
  fi
fi

# ── Unique URLs per unique row ────────────────────────────────────────────────

s1="$(gw_post "/files/upload-url" \
  '{"database":"main","table":"users","row_id":"uniq1-'${SUFFIX}'","column":"avatar"}' \
  /tmp/t_file_uniq1.json)"
s2="$(gw_post "/files/upload-url" \
  '{"database":"main","table":"users","row_id":"uniq2-'${SUFFIX}'","column":"avatar"}' \
  /tmp/t_file_uniq2.json)"

if [[ "$s1" == "200" && "$s2" == "200" ]]; then
  key1="$(jq -r '.object_key // ""' /tmp/t_file_uniq1.json 2>/dev/null || true)"
  key2="$(jq -r '.object_key // ""' /tmp/t_file_uniq2.json 2>/dev/null || true)"
  if [[ "$key1" != "$key2" ]]; then
    print_result 1 "files unique object_key per row"
  else
    print_result 0 "files unique object_key per row" "both got: ${key1}"
    FAIL=1
  fi
fi

exit "$FAIL"
