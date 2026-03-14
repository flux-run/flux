#!/usr/bin/env bash
# build.sh — Build hello.wasm by embedding handler.php into php-8.2.wasm.
#
# Usage:  ./build.sh [PHP_WASM_PATH]
#   PHP_WASM_PATH defaults to /tmp/php-8.2.6.wasm (downloaded by download-php.sh)
#
# Output: functions/hello/hello.wasm
#
# How it works:
#   The Flux WASM executor reads a custom WASM section named "flux.wasi-args"
#   from the module bytes.  The section payload is NUL-separated argv strings
#   (e.g. "php\0-r\0<script>\0").  Those strings are then served to the WASM
#   module via the WASI args_get / args_sizes_get syscalls so that the PHP
#   interpreter knows what code to run.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PHP_WASM="${1:-/tmp/php-8.2.6.wasm}"
HANDLER_PHP="${SCRIPT_DIR}/functions/hello/handler.php"
OUT_WASM="${SCRIPT_DIR}/functions/hello/hello.wasm"

if [[ ! -f "${PHP_WASM}" ]]; then
  echo "ERROR: PHP WASM binary not found at ${PHP_WASM}"
  echo "Run:  curl -L -o /tmp/php-8.2.6.wasm \\"
  echo "        https://github.com/vmware-labs/webassembly-language-runtimes/releases/download/php%2F8.2.6%2B20230714-11be424/php-8.2.6-wasmedge.wasm"
  exit 1
fi

# Strip comments + blank lines, collapse to a single semicolon-separated line
# so we can pass the entire program as one -r argument.
PHP_CODE=$(grep -v '^[[:space:]]*//' "${HANDLER_PHP}" | grep -v '^[[:space:]]*$' | grep -v '^<?php' | grep -v '^?>' | tr '\n' ' ' | sed 's/  */ /g' | sed 's/^ //;s/ $//')

python3 - "${PHP_WASM}" "${OUT_WASM}" "${PHP_CODE}" <<'PYEOF'
import sys, struct

def leb128_encode(n: int) -> bytes:
    result = []
    while True:
        byte = n & 0x7F
        n >>= 7
        if n:
            byte |= 0x80
        result.append(byte)
        if not n:
            break
    return bytes(result)

source_path, out_path, php_code = sys.argv[1], sys.argv[2], sys.argv[3]

with open(source_path, "rb") as f:
    php_wasm = f.read()

# Build NUL-separated argv: argv[0]=php  argv[1]=-r  argv[2]=<php code>
# PHP inline-code mode: `php -r '<code>'`  (no opening <?php tag needed with -r)
argv_content = b"php\x00-r\x00" + php_code.encode("utf-8") + b"\x00"

# Custom WASM section layout:
#   byte   0x00          (section id = 0 = custom)
#   leb128 section_size  (byte length of everything that follows)
#   leb128 name_length
#   bytes  name          ("flux.wasi-args")
#   bytes  content       (NUL-separated argv)
name = b"flux.wasi-args"
section_payload = leb128_encode(len(name)) + name + argv_content
section = bytes([0x00]) + leb128_encode(len(section_payload)) + section_payload

with open(out_path, "wb") as f:
    f.write(php_wasm + section)

total = len(php_wasm) + len(section)
print(f"[build.sh] wrote {out_path} ({total:,} bytes, custom section {len(section)} bytes)")
print(f"[build.sh] argv[0]=php  argv[1]=-r  argv[2]={php_code[:60]}{'...' if len(php_code)>60 else ''}")
PYEOF

echo "[build.sh] Done — ${OUT_WASM}"
