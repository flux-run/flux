#!/usr/bin/env bash
# setup-external-tests.sh
#
# Clones / copies the external test repositories that the compatibility runners
# need. All repos are placed under runtime/external-tests/.
#
# Usage:
#   bash runtime/scripts/setup-external-tests.sh all
#   bash runtime/scripts/setup-external-tests.sh 262         # test262 only
#   bash runtime/scripts/setup-external-tests.sh node        # node-core only
#   bash runtime/scripts/setup-external-tests.sh wpt         # web-platform only
#
# Requirements: git, bash ≥ 3.2

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUNTIME_DIR="$(dirname "$SCRIPT_DIR")"
EXTERNAL_DIR="$RUNTIME_DIR/external-tests"

TARGET="${1:-all}"

# ── Colour helpers ────────────────────────────────────────────────────────────
GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RED='\033[0;31m'; NC='\033[0m'
info()    { echo -e "${GREEN}▶${NC}  $*"; }
warn()    { echo -e "${YELLOW}⚠${NC}  $*"; }
success() { echo -e "${GREEN}✓${NC}  $*"; }
err()     { echo -e "${RED}✗${NC}  $*" >&2; }

# ── test262 ───────────────────────────────────────────────────────────────────
setup_test262() {
  local dest="$EXTERNAL_DIR/test262"
  if [[ -d "$dest/test" ]]; then
    warn "test262 already present at $dest — skipping (run 'git pull' inside to update)"
    return 0
  fi
  info "Cloning test262 (shallow, ~100 MB)…"
  mkdir -p "$EXTERNAL_DIR"
  git clone --depth 1 https://github.com/tc39/test262 "$dest"
  success "test262 ready at $dest"
  echo "  Tests: $(find "$dest/test" -name '*.js' | wc -l | tr -d ' ') files"
}

# ── node-core ─────────────────────────────────────────────────────────────────
setup_node_core() {
  local dest="$EXTERNAL_DIR/node-core"
  if [[ -d "$dest/parallel" ]]; then
    warn "node-core already present at $dest — skipping"
    return 0
  fi
  info "Fetching Node.js test/parallel and test/sequential (sparse checkout)…"
  mkdir -p "$EXTERNAL_DIR"
  local tmp; tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT

  git clone \
    --depth 1 \
    --filter=blob:none \
    --no-checkout \
    https://github.com/nodejs/node "$tmp/node-src"

  pushd "$tmp/node-src" >/dev/null
  git sparse-checkout init --cone
  git sparse-checkout set test/parallel test/sequential
  git checkout
  popd >/dev/null

  mkdir -p "$dest"
  cp -r "$tmp/node-src/test/parallel"   "$dest/parallel"
  cp -r "$tmp/node-src/test/sequential" "$dest/sequential"

  success "node-core ready at $dest"
  echo "  parallel:   $(ls "$dest/parallel"   | wc -l | tr -d ' ') files"
  echo "  sequential: $(ls "$dest/sequential" | wc -l | tr -d ' ') files"
}

# ── web-platform ──────────────────────────────────────────────────────────────
setup_wpt() {
  local dest="$EXTERNAL_DIR/web-platform"
  if [[ -d "$dest/url" ]]; then
    warn "web-platform tests already present at $dest — skipping"
    return 0
  fi
  info "Fetching Web Platform Tests (url, fetch, encoding — sparse checkout)…"
  mkdir -p "$EXTERNAL_DIR"

  git clone \
    --depth 1 \
    --filter=blob:none \
    --sparse \
    https://github.com/web-platform-tests/wpt "$dest"

  pushd "$dest" >/dev/null
  git sparse-checkout set url fetch encoding
  popd >/dev/null

  success "web-platform ready at $dest"
  for suite in url fetch encoding; do
    local count
    count=$(find "$dest/$suite" -name '*.js' 2>/dev/null | wc -l | tr -d ' ')
    echo "  ${suite}/: ${count} files"
  done
}

# ── dispatch ──────────────────────────────────────────────────────────────────
echo ""
echo "Flux External Test Setup"
echo "========================"
echo "Target: $EXTERNAL_DIR"
echo ""

case "$TARGET" in
  all)
    setup_test262
    echo ""
    setup_node_core
    echo ""
    setup_wpt
    ;;
  262|test262)
    setup_test262
    ;;
  node|node-core)
    setup_node_core
    ;;
  wpt|web|web-platform)
    setup_wpt
    ;;
  *)
    err "Unknown target: $TARGET"
    echo "Usage: $0 [all|262|node|wpt]"
    exit 1
    ;;
esac

echo ""
echo "Done. Run 'cd runtime/runners && npm run test:all' to execute all suites."
