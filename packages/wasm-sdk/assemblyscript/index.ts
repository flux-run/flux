/**
 * Fluxbase WASM SDK — AssemblyScript
 *
 * Import this module in your AssemblyScript function to get access to
 * Fluxbase host capabilities.
 *
 * ## Build
 * ```
 * npx asc assembly/index.ts \
 *     --target release \
 *     --outFile build/handler.wasm \
 *     --exportRuntime
 * ```
 *
 * ## ABI
 * The runtime calls `handle(payloadPtr, payloadLen): i32`.
 * Return a pointer where memory looks like: [4-byte u32 LE len][UTF-8 JSON].
 * JSON must have `{"output": ...}` or `{"error": "..."}`.
 */

// ── Host imports ──────────────────────────────────────────────────────────────

@external("fluxbase", "log")
declare function __flux_log(level: i32, ptr: i32, len: i32): void;

@external("fluxbase", "secrets_get")
declare function __flux_secrets_get(
  keyPtr: i32, keyLen: i32,
  outPtr: i32, outMax: i32
): i32;

@external("fluxbase", "http_fetch")
declare function __flux_http_fetch(
  reqPtr: i32, reqLen: i32,
  outPtr: i32, outMax: i32
): i32;

// ── Memory export (required by ABI) ──────────────────────────────────────────

export function __flux_alloc(size: i32): i32 {
  return heap.alloc(size) as i32;
}

// ── Log helpers ───────────────────────────────────────────────────────────────

/** Emit an info-level log message. */
export function log(msg: string): void {
  const encoded = String.UTF8.encode(msg);
  __flux_log(1, changetype<i32>(encoded), encoded.byteLength);
}

/** Emit a log message at an explicit level (1=info, 2=warn, 3=error). */
export function logLevel(level: i32, msg: string): void {
  const encoded = String.UTF8.encode(msg);
  __flux_log(level, changetype<i32>(encoded), encoded.byteLength);
}

// ── Secret helper ─────────────────────────────────────────────────────────────

/**
 * Retrieve a secret by key.
 * Returns an empty string if the key is not found.
 */
export function getSecret(key: string): string {
  const keyBytes = String.UTF8.encode(key);
  const outBuf   = new ArrayBuffer(4096);
  const n = __flux_secrets_get(
    changetype<i32>(keyBytes), keyBytes.byteLength,
    changetype<i32>(outBuf),   4096,
  );
  if (n <= 0) return "";
  return String.UTF8.decodeUnsafe(changetype<i32>(outBuf), n);
}

// ── HTTP fetch helper ─────────────────────────────────────────────────────────

/**
 * Perform an outbound HTTP request via the Fluxbase host.
 *
 * `requestJson` must be a JSON string matching:
 * `{"method":"GET","url":"https://...","headers":{},"body":"<base64>"}`
 *
 * Returns the response JSON string:
 * `{"status":200,"headers":{},"body":"<base64>"}`
 * or an empty string on error.
 */
export function httpFetch(requestJson: string): string {
  const reqBytes = String.UTF8.encode(requestJson);
  const outBuf   = new ArrayBuffer(65536);
  const n = __flux_http_fetch(
    changetype<i32>(reqBytes), reqBytes.byteLength,
    changetype<i32>(outBuf),   65536,
  );
  if (n <= 0) return "";
  return String.UTF8.decodeUnsafe(changetype<i32>(outBuf), n);
}

// ── Result builder ────────────────────────────────────────────────────────────

/**
 * Encode a JSON result string into the `[4-byte len][json]` layout expected
 * by the runtime and return the pointer.
 *
 * Call this at the end of every `handle()` export.
 *
 * @param json A complete JSON string, e.g. `{"output":{"message":"Hi!"}}`
 */
export function writeResult(json: string): i32 {
  const encoded = String.UTF8.encode(json);
  const len     = encoded.byteLength;
  const ptr     = heap.alloc(4 + len) as i32;
  store<u32>(ptr, len as u32);
  memory.copy(ptr + 4, changetype<i32>(encoded), len);
  return ptr;
}

/**
 * Convenience wrapper — write a success result.
 *
 * `output` should be a JSON-serialised value string, e.g. `{"message":"Hi"}`.
 */
export function writeOutput(outputJson: string): i32 {
  return writeResult('{"output":' + outputJson + "}");
}

/**
 * Convenience wrapper — write an error result.
 */
export function writeError(message: string): i32 {
  // Simple escaping for the error message string.
  const escaped = message
    .replaceAll("\\", "\\\\")
    .replaceAll('"', '\\"');
  return writeResult('{"error":"' + escaped + '"}');
}
