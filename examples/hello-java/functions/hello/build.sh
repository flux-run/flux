#!/usr/bin/env bash
# Build Handler.java → hello.wasm via TeaVM
#
# Requirements:
#   Java 17+  https://adoptium.net
#   Gradle 7+ (wrapper included — no separate install needed)

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "Building Handler.java → hello.wasm (TeaVM)..."
./gradlew generateWasm
echo "Done: $(ls -lh hello.wasm | awk '{print $5, $9}')"
