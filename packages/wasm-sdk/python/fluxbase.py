"""
fluxbase — Fluxbase WASM SDK for Python (py2wasm)

py2wasm compiles a subset of Python to WebAssembly. This module provides
typed helpers that mirror the ABI contract, plus stub declarations for the
host imports so the source parses cleanly under both py2wasm and a normal
CPython interpreter.

## Build
    pip install py2wasm
    py2wasm handler.py -o handler.wasm

## ABI contract
The runtime calls:
    handle(payload_ptr: int, payload_len: int) -> int

The returned int is a pointer to [4-byte u32 LE length][UTF-8 JSON].
JSON must contain {"output": ...} or {"error": "..."}.

## Limitations
py2wasm supports only a subset of Python (no stdlib imports inside WASM).
This SDK provides self-contained helpers that work within those constraints.
"""

from __future__ import annotations

import json

# ---------------------------------------------------------------------------
# py2wasm host-import stubs
# These are replaced by the real host functions at compile time.  They exist
# here only so the source file is valid Python and can be tested locally.
# ---------------------------------------------------------------------------

def __flux_host_log(level: int, ptr: int, length: int) -> None:
    """Host import: fluxbase.log"""
    # CPython fallback — does nothing; real call happens inside WASM.
    pass  # pragma: no cover


def __flux_host_secrets_get(key_ptr: int, key_len: int, out_ptr: int, out_max: int) -> int:
    """Host import: fluxbase.secrets_get"""
    return -1  # pragma: no cover


def __flux_host_http_fetch(req_ptr: int, req_len: int, out_ptr: int, out_max: int) -> int:
    """Host import: fluxbase.http_fetch"""
    return -1  # pragma: no cover


# ---------------------------------------------------------------------------
# Memory helpers (provided by py2wasm runtime as builtins at compile time)
# ---------------------------------------------------------------------------

def _alloc(size: int) -> int:       # noqa: E301
    """Allocate `size` bytes; returns pointer."""
    raise NotImplementedError("only valid inside WASM")  # pragma: no cover


def _write_bytes(ptr: int, data: bytes) -> None:
    """Write `data` starting at `ptr`."""
    raise NotImplementedError("only valid inside WASM")  # pragma: no cover


def _read_bytes(ptr: int, length: int) -> bytes:
    """Read `length` bytes starting at `ptr`."""
    raise NotImplementedError("only valid inside WASM")  # pragma: no cover


# ---------------------------------------------------------------------------
# py2wasm export decorator (marks a Python function as a WASM export)
# At runtime inside py2wasm this is a real decorator; here it is a no-op.
# ---------------------------------------------------------------------------

def export(fn):  # type: ignore[no-untyped-def]
    return fn


# ---------------------------------------------------------------------------
# Required ABI export
# ---------------------------------------------------------------------------

@export
def __flux_alloc(size: int) -> int:
    return _alloc(size)


# ---------------------------------------------------------------------------
# SDK helpers
# ---------------------------------------------------------------------------

_LOG_INFO  = 1
_LOG_WARN  = 2
_LOG_ERROR = 3


def log(message: str, level: int = _LOG_INFO) -> None:
    """Emit a log line to the Fluxbase host."""
    encoded = message.encode("utf-8")
    ptr = _alloc(len(encoded))
    _write_bytes(ptr, encoded)
    __flux_host_log(level, ptr, len(encoded))


def get_secret(key: str) -> str | None:
    """
    Retrieve a secret by key.
    Returns None if the key is not found.
    """
    key_bytes = key.encode("utf-8")
    key_ptr = _alloc(len(key_bytes))
    _write_bytes(key_ptr, key_bytes)

    out_max = 4096
    out_ptr = _alloc(out_max)
    n = __flux_host_secrets_get(key_ptr, len(key_bytes), out_ptr, out_max)
    if n < 0:
        return None
    return _read_bytes(out_ptr, n).decode("utf-8")


def http_fetch(method: str, url: str, headers: dict | None = None, body: str = "") -> dict:
    """
    Perform an outbound HTTP request via the Fluxbase host.

    Returns a dict with keys: ``status`` (int), ``headers`` (dict), ``body`` (str, base64).
    """
    req = {"method": method, "url": url}
    if headers:
        req["headers"] = headers
    if body:
        req["body"] = body

    req_bytes = json.dumps(req).encode("utf-8")
    req_ptr = _alloc(len(req_bytes))
    _write_bytes(req_ptr, req_bytes)

    out_max = 65536
    out_ptr = _alloc(out_max)
    n = __flux_host_http_fetch(req_ptr, len(req_bytes), out_ptr, out_max)
    if n < 0:
        return {"error": f"http_fetch failed: {n}"}
    return json.loads(_read_bytes(out_ptr, n).decode("utf-8"))


def write_result(result_json: str) -> int:
    """
    Encode `result_json` into the ``[4-byte LE len][bytes]`` layout and
    return the pointer.  Call this as the last statement in `handle()`.
    """
    encoded = result_json.encode("utf-8")
    length  = len(encoded)
    header  = bytes([
        length & 0xff,
        (length >> 8)  & 0xff,
        (length >> 16) & 0xff,
        (length >> 24) & 0xff,
    ])
    buf_ptr = _alloc(4 + length)
    _write_bytes(buf_ptr, header + encoded)
    return buf_ptr


def success(output: object) -> int:
    """Return a successful handler result from a Python object (JSON-serialised)."""
    return write_result(json.dumps({"output": output}))


def error(message: str) -> int:
    """Return a failed handler result."""
    return write_result(json.dumps({"error": message}))
