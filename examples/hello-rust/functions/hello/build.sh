#!/usr/bin/env bash
# Build Rust → hello.wasm (wasm32-wasip1)
#
# Requirements:
#   Rust + Cargo  https://rustup.rs
#   wasm32-wasip1 target: rustup target add wasm32-wasip1

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

if ! rustup target list --installed | grep -q wasm32-wasip1; then
  echo "Adding wasm32-wasip1 target..."
  rustup target add wasm32-wasip1
fi

echo "Building hello → hello.wasm..."
cargo build --target wasm32-wasip1 --release
cp target/wasm32-wasip1/release/hello.wasm hello.wasm
echo "Done: $(ls -lh hello.wasm | awk '{print $5, $9}')"
