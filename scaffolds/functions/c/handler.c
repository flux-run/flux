// {name} — Flux function (compiled to WASM via wasi-sdk)
// Build: see Makefile  (requires wasi-sdk — https://github.com/WebAssembly/wasi-sdk)
#include <stdint.h>

// Static response in WASM linear memory.
static const char RESP[] = "{\"ok\":true}";
#define RESP_LEN ((uint32_t)(sizeof(RESP) - 1))

__attribute__((export_name("{name}_handler")))
uint64_t {name}_handler(uint32_t input_ptr, uint32_t input_len) {
    (void)input_ptr;
    (void)input_len;
    // Return (pointer << 32) | length packed into uint64.
    return ((uint64_t)(uintptr_t)RESP << 32) | RESP_LEN;
}
