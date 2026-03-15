#!/usr/bin/env bash
# scripts/install-local.sh
# Build and install the flux CLI and server binaries locally.
#
# Usage:
#   ./scripts/install-local.sh            # install both cli + server
#   ./scripts/install-local.sh --cli      # install cli only
#   ./scripts/install-local.sh --server   # install server only

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

INSTALL_CLI=true
INSTALL_SERVER=true

# ─── Argument parsing ───────────────────────────────────────────────────────
while [[ "$#" -gt 0 ]]; do
  case $1 in
    --cli)    INSTALL_SERVER=false ;;
    --server) INSTALL_CLI=false ;;
    *) echo "Unknown option: $1"; echo "Usage: $0 [--cli] [--server]"; exit 1 ;;
  esac
  shift
done

cd "$ROOT"

VERSION=$(grep '^version' cli/Cargo.toml | head -1 | awk -F'"' '{print $2}')
echo "flux v$VERSION — local install"
echo ""

# ─── CLI ────────────────────────────────────────────────────────────────────
if $INSTALL_CLI; then
  echo "▶ Installing flux CLI..."
  CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
  rm -f "$CARGO_BIN/flux"
  # Touch main entry points to bust cargo's release artifact cache so the
  # install always recompiles from current source, not a stale cached binary.
  touch "$ROOT/cli/src/main.rs" "$ROOT/cli/src/dev.rs"
  SQLX_OFFLINE=true cargo install --path cli --offline --force
  # Sync to any other PATH locations of an older flux binary (e.g. /usr/local/bin)
  # so the newly built version is always the one that runs.
  while IFS= read -r other; do
    if [[ "$other" != "$CARGO_BIN/flux" && -f "$other" ]]; then
      echo "  → syncing to $other"
      cp "$CARGO_BIN/flux" "$other"
    fi
  done < <(which -a flux 2>/dev/null || true)
  echo "✓ flux CLI installed → $CARGO_BIN/flux"
  echo ""
fi

# ─── Server ─────────────────────────────────────────────────────────────────
if $INSTALL_SERVER; then
  echo "▶ Building dashboard (required by server)..."
  (cd "$ROOT/dashboard" && npm run build)
  echo "▶ Installing server..."
  CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
  rm -f "$CARGO_BIN/server"
  touch "$ROOT/server/src/main.rs"
  SQLX_OFFLINE=true cargo install --path server --offline --force
  echo "✓ server installed → $CARGO_BIN/server"
  echo ""
fi

echo "Done. Run 'flux --version' to verify."
