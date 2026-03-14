#!/usr/bin/env bash
# Build main.go → hello.wasm (GOOS=wasip1 GOARCH=wasm)
#
# Requirements:
#   Go 1.21+  https://go.dev/doc/install

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Building main.go → hello.wasm (wasip1)..."
GOOS=wasip1 GOARCH=wasm go build -o hello.wasm .
echo "Done: $(ls -lh hello.wasm | awk '{print $5, $9}')"
