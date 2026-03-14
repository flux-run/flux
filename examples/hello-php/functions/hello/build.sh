#!/usr/bin/env bash
# Build handler.php → hello.wasm
#
# The PHP WASM binary (php-8.2.6-wasmedge.wasm) from vmware-labs/webassembly-
# language-runtimes is downloaded and the PHP script is embedded as a
# flux.wasi-args custom WASM section (NUL-separated argv bytes).
#
# The Flux runtime reads the section and passes ["php", "-r", "<script>"] as
# WASI argv before calling _start — no argv is needed at runtime.
#
# Requirements:
#   curl, python3 (for the section-embed helper)
#   wabt (wasm-strip)  — optional, reduces binary size

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

PHP_WASM_URL="https://github.com/vmware-labs/webassembly-language-runtimes/releases/download/php%2F8.2.6%2B20230901-7b2e9e3/php-8.2.6-wasmedge.wasm"
PHP_WASM="$SCRIPT_DIR/php.wasm"

if [[ ! -f "$PHP_WASM" ]]; then
  echo "Downloading php-8.2.6-wasmedge.wasm (~13 MB)..."
  curl -fSL "$PHP_WASM_URL" -o "$PHP_WASM"
fi

SCRIPT=$(cat handler.php)

python3 - <<EOF
import struct, sys

with open("$PHP_WASM", "rb") as f:
    wasm = bytearray(f.read())

# Build flux.wasi-args custom section: NUL-separated argv bytes
section_name = b"flux.wasi-args"
argv = ["php", "-r", """$SCRIPT"""]
payload = b"\x00".join(a.encode() for a in argv)

def encode_uleb128(n):
    buf = []
    while True:
        b = n & 0x7F
        n >>= 7
        if n:
            buf.append(b | 0x80)
        else:
            buf.append(b)
            break
    return bytes(buf)

name_bytes = encode_uleb128(len(section_name)) + section_name
content = encode_uleb128(len(payload)) + payload
section_body = name_bytes + content
section = bytes([0x00]) + encode_uleb128(len(section_body)) + section_body

with open("hello.wasm", "wb") as f:
    f.write(bytes(wasm) + section)

print(f"Done: embedded {len(payload)} bytes of argv into hello.wasm")
EOF
echo "Built: $(ls -lh hello.wasm | awk '{print $5, $9}')"
