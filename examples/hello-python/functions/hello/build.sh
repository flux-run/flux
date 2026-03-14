#!/usr/bin/env bash
# Build handler.py → hello.wasm via py2wasm (Nuitka)
#
# Requirements:
#   Python 3.11 (py2wasm does not support 3.12+)
#   pip install py2wasm
#
# Install:
#   brew install python@3.11
#   python3.11 -m venv .venv && .venv/bin/pip install py2wasm
#
# Then run:
#   ./build.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Find py2wasm — prefer a local .venv, fall back to PATH
if [[ -x "$SCRIPT_DIR/.venv/bin/py2wasm" ]]; then
  PY2WASM="$SCRIPT_DIR/.venv/bin/py2wasm"
elif command -v py2wasm &>/dev/null; then
  PY2WASM="py2wasm"
else
  echo "error: py2wasm not found. Run: python3.11 -m venv .venv && .venv/bin/pip install py2wasm" >&2
  exit 1
fi

echo "Building handler.py → hello.wasm (Nuitka compile, ~60s first run)..."
"$PY2WASM" "$SCRIPT_DIR/handler.py" -o "$SCRIPT_DIR/hello.wasm"
echo "Done: $(ls -lh "$SCRIPT_DIR/hello.wasm" | awk '{print $5, $9}')"
