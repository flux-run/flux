# hello_wasm_py — Fluxbase "hello world" function in Python (py2wasm).
#
# Build:
#   pip install py2wasm
#   py2wasm handler.py -o handler.wasm
#
# Deploy:
#   flux deploy   (runs the build command from flux.json, then uploads handler.wasm)
#
# Invoke:
#   flux invoke hello_wasm_py '{"name":"Alice"}'
#   # → {"message":"Hello Alice!"}
#
# Note: py2wasm supports a restricted subset of Python. Only basic arithmetic,
# string operations, and the builtins used below are guaranteed to work.

import json

# ── py2wasm memory builtins (replaced at compile time) ───────────────────────

def __alloc(size: int) -> int: ...
def __write_bytes(ptr: int, data: bytes) -> None: ...
def __read_bytes(ptr: int, length: int) -> bytes: ...

# ── Fluxbase host imports (replaced at compile time) ─────────────────────────

def __flux_log(level: int, ptr: int, length: int) -> None: ...

# ── py2wasm export marker ─────────────────────────────────────────────────────

def export(fn):
    return fn

# ── ABI exports ───────────────────────────────────────────────────────────────

@export
def __flux_alloc(size: int) -> int:
    return __alloc(size)

# ── Helpers ───────────────────────────────────────────────────────────────────

def _log(msg: str) -> None:
    b = msg.encode("utf-8")
    ptr = __alloc(len(b))
    __write_bytes(ptr, b)
    __flux_log(1, ptr, len(b))

def _write_result(result_json: str) -> int:
    encoded = result_json.encode("utf-8")
    length  = len(encoded)
    header  = bytes([
        length & 0xff,
        (length >> 8)  & 0xff,
        (length >> 16) & 0xff,
        (length >> 24) & 0xff,
    ])
    buf_ptr = __alloc(4 + length)
    __write_bytes(buf_ptr, header + encoded)
    return buf_ptr

# ── Handler ───────────────────────────────────────────────────────────────────

@export
def handle(payload_ptr: int, payload_len: int) -> int:
    payload_bytes = __read_bytes(payload_ptr, payload_len)
    data = json.loads(payload_bytes.decode("utf-8"))

    _log("hello_wasm_py: executing")

    name = data.get("name", "world")
    result = json.dumps({"output": {"message": "Hello " + name + "!"}})
    return _write_result(result)
