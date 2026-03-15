// hello — Flux function (AssemblyScript → WASM)
//
// ABI contract:
//   Host provides: flux.log(level: i32, msg_ptr: i32, msg_len: i32)
//   Module exports:
//     memory          — linear memory
//     __flux_alloc(size: i32) → i32   — allocate `size` bytes, return pointer
//     handle(ptr: i32, len: i32) → i32 — main entry; return pointer to result
//
//   Result layout at returned pointer:
//     [4 bytes u32 LE = JSON length][JSON bytes]
//     JSON must have "output" or "error" key at top level.
//
// Build: /tmp/node_modules/.bin/asc index.ts --target release --exportRuntime -o index.wasm

// ── Host import ──────────────────────────────────────────────────────────────

@external("flux", "log")
declare function flux_log(level: i32, msg_ptr: i32, msg_len: i32): void;

function log_info(msg: string): void {
  const encoded = String.UTF8.encode(msg);
  flux_log(1, changetype<i32>(encoded), encoded.byteLength);
}

// ── Allocator ────────────────────────────────────────────────────────────────

// Simple bump allocator backed by a static arena.
// AssemblyScript's --exportRuntime provides __new; this simpler version avoids
// needing runtime exports for the host.

const ARENA_BASE: i32 = 65536; // one page above the stack
let   arena_bump: i32 = ARENA_BASE;

export function __flux_alloc(size: i32): i32 {
  const ptr = arena_bump;
  // Align to 8 bytes to be safe.
  arena_bump = (arena_bump + size + 7) & ~7;
  return ptr;
}

// ── Result writer ─────────────────────────────────────────────────────────────

function write_result(json: string): i32 {
  const encoded = String.UTF8.encode(json);
  const json_len: i32 = encoded.byteLength;
  // Allocate 4 (u32 LE length header) + json bytes
  const total = 4 + json_len;
  const ptr = __flux_alloc(total);
  // Write u32 LE length header
  store<u8>(ptr + 0, (json_len       ) & 0xff);
  store<u8>(ptr + 1, (json_len >>  8 ) & 0xff);
  store<u8>(ptr + 2, (json_len >> 16 ) & 0xff);
  store<u8>(ptr + 3, (json_len >> 24 ) & 0xff);
  // Write JSON bytes
  memory.copy(ptr + 4, changetype<i32>(encoded), json_len);
  return ptr;
}

// ── Handler ──────────────────────────────────────────────────────────────────

export function handle(input_ptr: i32, input_len: i32): i32 {
  log_info("hello-assemblyscript handler invoked");
  return write_result(`{"output":{"ok":true}}`);
}
