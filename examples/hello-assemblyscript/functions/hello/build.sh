#!/usr/bin/env bash
# Build index.ts → index.wasm via AssemblyScript (asc)
#
# Requirements:
#   Node.js 18+  https://nodejs.org
#
# Install deps and build:
#   npm install
#   ./build.sh

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if [[ ! -d node_modules ]]; then
  echo "Installing AssemblyScript..."
  npm install
fi

echo "Building index.ts → index.wasm..."
npx asc index.ts --target release
echo "Done: $(ls -lh index.wasm | awk '{print $5, $9}')"
