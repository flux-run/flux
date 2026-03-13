#!/usr/bin/env bash
# function_test.sh — comprehensive function invocation tests
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
source "$DIR/common.sh"
common_init

FAIL=0

# ── Basic invocation ──────────────────────────────────────────────────────────

s="$(gw_post "/${FUNCTION_NAME}" '{"message":"hello"}' \
  /tmp/t_fn_basic.json /tmp/t_fn_basic_hdr.txt)"
assert_status_and_jq "200" "$s" \
  'has("duration_ms") or has("result") or ((.data // {}) | has("duration_ms"))' \
  /tmp/t_fn_basic.json "functions basic invoke" FAIL

# x-request-id header must be present
assert_header_present "x-request-id" /tmp/t_fn_basic_hdr.txt "functions x-request-id header" FAIL

# ── Empty payload ─────────────────────────────────────────────────────────────

s="$(gw_post "/${FUNCTION_NAME}" '{}' /tmp/t_fn_empty.json)"
if [[ "$s" == "200" ]]; then
  print_result 1 "functions empty payload"
else
  print_result 0 "functions empty payload" "HTTP ${s}"
  FAIL=1
fi

# ── Nested payload ────────────────────────────────────────────────────────────

s="$(gw_post "/${FUNCTION_NAME}" \
  '{"user":{"name":"Alice","age":30},"tags":["a","b","c"]}' \
  /tmp/t_fn_nested.json)"
if [[ "$s" == "200" ]]; then
  print_result 1 "functions nested payload"
else
  print_result 0 "functions nested payload" "HTTP ${s}"
  FAIL=1
fi

# ── Unicode payload ───────────────────────────────────────────────────────────

s="$(gw_post "/${FUNCTION_NAME}" \
  '{"message":"héllo wörld 🌍"}' \
  /tmp/t_fn_unicode.json)"
if [[ "$s" == "200" ]]; then
  print_result 1 "functions unicode payload"
else
  print_result 0 "functions unicode payload" "HTTP ${s}"
  FAIL=1
fi

# ── Numeric / boolean values ──────────────────────────────────────────────────

s="$(gw_post "/${FUNCTION_NAME}" \
  '{"count":42,"active":true,"score":3.14,"nothing":null}' \
  /tmp/t_fn_types.json)"
if [[ "$s" == "200" ]]; then
  print_result 1 "functions mixed type values"
else
  print_result 0 "functions mixed type values" "HTTP ${s}"
  FAIL=1
fi

# ── Repeated invocations (cache / warm path) ──────────────────────────────────

for i in 1 2 3; do
  s="$(gw_post "/${FUNCTION_NAME}" "{\"call\":${i}}" /tmp/t_fn_rep_${i}.json)"
  if [[ "$s" != "200" ]]; then
    print_result 0 "functions repeat call ${i}" "HTTP ${s}"
    FAIL=1
  fi
done
print_result 1 "functions 3 repeat invocations"

# ── Response content-type ─────────────────────────────────────────────────────

s="$(gw_post "/${FUNCTION_NAME}" '{"message":"ct-check"}' \
  /tmp/t_fn_ct.json /tmp/t_fn_ct_hdr.txt)"
if [[ "$s" == "200" ]]; then
  ct="$(http_header_value /tmp/t_fn_ct_hdr.txt content-type || true)"
  if printf '%s' "$ct" | grep -qi 'application/json'; then
    print_result 1 "functions response content-type json"
  else
    print_result 0 "functions response content-type json" "got: ${ct:-missing}"
    FAIL=1
  fi
fi

# ── Duration in response ──────────────────────────────────────────────────────

s="$(gw_post "/${FUNCTION_NAME}" '{"message":"dur"}' /tmp/t_fn_dur.json)"
if [[ "$s" == "200" ]]; then
  dur="$(jq '(.duration_ms // (.data // {}).duration_ms // -1)' /tmp/t_fn_dur.json 2>/dev/null || echo -1)"
  if [[ "$dur" != "-1" ]]; then
    print_result 1 "functions duration_ms in response"
  else
    print_result 0 "functions duration_ms in response" "body=$(jq -c '.' /tmp/t_fn_dur.json 2>/dev/null | head -c 80)"
    FAIL=1
  fi
fi

# ── Non-existent function → 404 ───────────────────────────────────────────────

s="$(gw_post "/__nonexistent_fn_$(unique_suffix)__" '{}' /tmp/t_fn_404.json)"
assert_status "404" "$s" "functions unknown route → 404" FAIL

# ── Error response structure ──────────────────────────────────────────────────

if [[ "$s" == "404" ]]; then
  assert_jq 'has("error")' /tmp/t_fn_404.json "functions 404 error field" FAIL
fi

exit "$FAIL"
