# hello — Flux function (compiled to WASM via py2wasm)
# Build: py2wasm -i handler.py -o hello.wasm

import json

def handler(input_json: str) -> str:
    \"\"\"Entry point called by the Flux runtime.\"\"\"
    payload = json.loads(input_json)

    # TODO: implement hello
    result = {"ok": True}

    return json.dumps(result)
