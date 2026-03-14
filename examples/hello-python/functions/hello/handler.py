# hello — Flux function (compiled to WASM via py2wasm)
# Build: py2wasm -i handler.py -o hello.wasm
#
# Uses WASI stdin/stdout model: reads JSON from stdin, writes JSON to stdout.

import json
import sys

def handler(input_data: dict) -> dict:
    # TODO: implement hello
    return {"ok": True}

if __name__ == "__main__":
    raw = sys.stdin.read()
    try:
        payload = json.loads(raw) if raw.strip() else {}
    except Exception:
        payload = {}
    result = handler(payload)
    sys.stdout.write(json.dumps(result))
