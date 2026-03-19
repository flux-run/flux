#!/bin/bash
# scripts/e2e/assertions.sh
# Assertion helpers for the Flux E2E user flow test.
# Source this file, do not run it directly.
#
# Usage:
#   source scripts/e2e/assertions.sh
#   assert_contains "flux trace output" "POSTGRES"
#   assert_exit_zero "flux replay $ID"

set -euo pipefail

PASS=0
FAIL=0
FAILURES=()

# ── Colour helpers ────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

pass() { echo -e "  ${GREEN}✔${NC} $1"; ((PASS++)); }
fail() { echo -e "  ${RED}✗${NC} $1"; FAILURES+=("$1"); ((FAIL++)); }
section() { echo -e "\n${CYAN}${BOLD}── $1 ──${NC}"; }

# assert_contains <string> <substring>
assert_contains() {
  local haystack="$1"
  local needle="$2"
  local label="${3:-contains '$needle'}"
  if echo "$haystack" | grep -qF "$needle"; then
    pass "$label"
  else
    fail "$label — output was: $(echo "$haystack" | head -3)"
  fi
}

# assert_not_contains <string> <substring>
assert_not_contains() {
  local haystack="$1"
  local needle="$2"
  local label="${3:-does NOT contain '$needle'}"
  if echo "$haystack" | grep -qF "$needle"; then
    fail "$label — unexpectedly found: $needle"
  else
    pass "$label"
  fi
}

# assert_exit_zero <cmd...>
assert_exit_zero() {
  local label="${1}"
  shift
  if "$@" >/dev/null 2>&1; then
    pass "$label"
  else
    fail "$label (exit non-zero)"
  fi
}

# assert_http_status <expected_code> <url>
assert_http_status() {
  local expected="$1"
  local url="$2"
  local label="${3:-HTTP $expected from $url}"
  local actual
  actual=$(curl -s -o /dev/null -w "%{http_code}" "$url" 2>/dev/null || echo "000")
  if [[ "$actual" == "$expected" ]]; then
    pass "$label"
  else
    fail "$label — got HTTP $actual"
  fi
}

# assert_json_field <json_string> <jq_filter> <expected_value>
assert_json_field() {
  local json="$1"
  local filter="$2"
  local expected="$3"
  local label="${4:-JSON $filter == $expected}"
  local actual
  actual=$(echo "$json" | jq -r "$filter" 2>/dev/null || echo "__jq_error__")
  if [[ "$actual" == "$expected" ]]; then
    pass "$label"
  else
    fail "$label — got: $actual"
  fi
}

# assert_equal <a> <b>
assert_equal() {
  local a="$1"
  local b="$2"
  local label="${3:-values are identical}"
  if [[ "$a" == "$b" ]]; then
    pass "$label"
  else
    fail "$label"$'\n'"    expected: $b"$'\n'"    got:      $a"
  fi
}

# assert_nonempty <string>
assert_nonempty() {
  local val="$1"
  local label="${2:-value is non-empty}"
  if [[ -n "$val" && "$val" != "null" && "$val" != "none" ]]; then
    pass "$label"
  else
    fail "$label — got empty/null value"
  fi
}

# wait_for_http <url> [max_seconds]
wait_for_http() {
  local url="$1"
  local max="${2:-20}"
  local i=0
  while ! curl -sf "$url" >/dev/null 2>&1; do
    sleep 1
    ((i++))
    if [[ $i -ge $max ]]; then
      fail "Timed out waiting for $url after ${max}s"
      return 1
    fi
  done
  pass "Service up at $url (waited ${i}s)"
}

# ── Summary printer ───────────────────────────────────────────────────────────
e2e_summary() {
  echo ""
  echo -e "${BOLD}╔═══════════════════════════════════════════════════════╗${NC}"
  echo -e "${BOLD}║           FLUX E2E USER FLOW — RESULTS               ║${NC}"
  echo -e "${BOLD}╠═══════════════════════════════════════════════════════╣${NC}"
  echo -e "${BOLD}║  ✔ Passed:  $(printf '%3d' $PASS)                                    ║${NC}"
  echo -e "${BOLD}║  ✗ Failed:  $(printf '%3d' $FAIL)                                    ║${NC}"
  echo -e "${BOLD}╠═══════════════════════════════════════════════════════╣${NC}"
  if [[ ${#FAILURES[@]} -gt 0 ]]; then
    echo -e "${BOLD}║  Failed assertions:                                   ║${NC}"
    for f in "${FAILURES[@]}"; do
      echo -e "  ${RED}→${NC} $f"
    done
    echo -e "${BOLD}╚═══════════════════════════════════════════════════════╝${NC}"
    echo -e "\n${RED}${BOLD}🚨 E2E FLOW FAILED: $FAIL assertion(s) did not pass. DO NOT RELEASE.${NC}\n"
    exit 1
  else
    echo -e "${BOLD}╚═══════════════════════════════════════════════════════╝${NC}"
    echo -e "\n${GREEN}${BOLD}✅ All $PASS E2E assertions passed. User flow is healthy.${NC}\n"
    exit 0
  fi
}
