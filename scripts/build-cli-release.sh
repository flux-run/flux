#!/usr/bin/env bash
# scripts/build-cli-release.sh
# Build the Fluxbase CLI for all supported platforms.
#
# Prerequisites (install once):
#   cargo install cross --git https://github.com/cross-rs/cross
#   brew install mingw-w64          # macOS — for Windows targets
#   rustup target add <target>      # for each target below
#
# Usage:
#   ./scripts/build-cli-release.sh
#   ./scripts/build-cli-release.sh --only linux   # subset

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OUT="$ROOT/dist/cli"
PACKAGE="cli"

# ─── Targets ────────────────────────────────────────────────────────────────
declare -A TARGET_NAMES=(
  ["aarch64-apple-darwin"]="flux-darwin-arm64"
  ["x86_64-apple-darwin"]="flux-darwin-amd64"
  ["aarch64-unknown-linux-gnu"]="flux-linux-arm64"
  ["x86_64-unknown-linux-gnu"]="flux-linux-amd64"
  ["aarch64-pc-windows-msvc"]="flux-windows-arm64.exe"
  ["x86_64-pc-windows-msvc"]="flux-windows-amd64.exe"
)

mkdir -p "$OUT"

# ─── Filter ─────────────────────────────────────────────────────────────────
FILTER="${1:-}"
build_target() {
  local target="$1"
  [[ -z "$FILTER" ]] && return 0
  [[ "$target" == *"$FILTER"* ]] && return 0
  return 1
}

# ─── Version ────────────────────────────────────────────────────────────────
VERSION=$(cargo metadata --format-version 1 --no-deps -q 2>/dev/null \
  | python3 -c "import sys,json; pkgs=json.load(sys.stdin)['packages']; \
    print(next(p['version'] for p in pkgs if p['name']=='cli'))" 2>/dev/null \
  || grep '^version' "$ROOT/cli/Cargo.toml" | head -1 | awk -F'"' '{print $2}')
echo "  Building flux v$VERSION"
echo ""

# ─── Build each target ──────────────────────────────────────────────────────
for TARGET in "${!TARGET_NAMES[@]}"; do
  NAME="${TARGET_NAMES[$TARGET]}"

  # Filter
  if [[ -n "$FILTER" ]] && [[ "$TARGET" != *"$FILTER"* ]]; then
    echo "  skip $TARGET"
    continue
  fi

  echo "  ▸ $TARGET → dist/cli/$NAME"

  # Prefer `cross` for Linux cross-compilation; fall back to cargo for native.
  if [[ "$TARGET" == *"linux"* ]]; then
    BUILDER=cross
  else
    BUILDER=cargo
  fi

  $BUILDER build --release --target "$TARGET" -p "$PACKAGE" 2>&1 \
    | grep -E "^error|Compiling cli|Finished" || true

  # Locate the output binary
  if [[ "$TARGET" == *"windows"* ]]; then
    SRC="$ROOT/target/$TARGET/release/cli.exe"
  else
    SRC="$ROOT/target/$TARGET/release/cli"
  fi

  if [[ -f "$SRC" ]]; then
    cp "$SRC" "$OUT/$NAME"
    SIZE=$(du -sh "$OUT/$NAME" | awk '{print $1}')
    echo "    ✔ $NAME ($SIZE)"
  else
    echo "    ✗ binary not found at $SRC — skipping"
  fi
  echo ""
done

# ─── Checksums ──────────────────────────────────────────────────────────────
echo "  Generating checksums..."
cd "$OUT"
sha256sum flux-* > sha256sums.txt 2>/dev/null || shasum -a 256 flux-* > sha256sums.txt
echo "  ✔ sha256sums.txt"
echo ""
echo "  Done. Binaries in dist/cli/"
ls -lh "$OUT"
