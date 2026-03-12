// hello_wasm_as — Fluxbase "hello world" function in AssemblyScript.
//
// Build:
//   npm install
//   npm run build
//
// Deploy:
//   flux deploy   (runs the build command from flux.json, then uploads build/handler.wasm)
//
// Invoke:
//   flux invoke hello_wasm_as '{"name":"Alice"}'
//   # → {"message":"Hello Alice!"}

// ── Fluxbase host imports ─────────────────────────────────────────────────────

@external("fluxbase", "log")
declare function __flux_log(level: i32, ptr: i32, len: i32): void;

// ── ABI ───────────────────────────────────────────────────────────────────────

export function __flux_alloc(size: i32): i32 {
  return heap.alloc(size) as i32;
}

function writeResult(json: string): i32 {
  const encoded = String.UTF8.encode(json);
  const len     = encoded.byteLength;
  const ptr     = heap.alloc(4 + len) as i32;
  store<u32>(ptr, len as u32);
  memory.copy(ptr + 4, changetype<i32>(encoded), len);
  return ptr;
}

function log(msg: string): void {
  const b = String.UTF8.encode(msg);
  __flux_log(1, changetype<i32>(b), b.byteLength);
}

// ── Handler ───────────────────────────────────────────────────────────────────

export function handle(payloadPtr: i32, payloadLen: i32): i32 {
  const payload = String.UTF8.decodeUnsafe(payloadPtr, payloadLen);
  log("hello_wasm_as: executing");

  // Naive JSON "name" extraction — for production use a proper JSON library.
  let name = "world";
  const nameIdx = payload.indexOf('"name"');
  if (nameIdx >= 0) {
    const colonIdx  = payload.indexOf(":", nameIdx);
    const quote1    = payload.indexOf('"', colonIdx + 1);
    const quote2    = payload.indexOf('"', quote1 + 1);
    if (quote1 >= 0 && quote2 > quote1) {
      name = payload.slice(quote1 + 1, quote2);
    }
  }

  return writeResult(`{"output":{"message":"Hello ${name}!"}}`);
}
