// hello — Flux function (compiled to WASM via wasi-sdk)
// Build: see Makefile  (requires wasi-sdk — https://github.com/WebAssembly/wasi-sdk)
#include <cstdint>

static const char RESP[] = "{\"ok\":true}";
constexpr uint32_t RESP_LEN = sizeof(RESP) - 1;

extern "C" {
    __attribute__((export_name("hello_handler")))
    uint64_t hello_handler(uint32_t input_ptr, uint32_t input_len) {
        (void)input_ptr;
        (void)input_len;
        return (static_cast<uint64_t>(reinterpret_cast<uintptr_t>(RESP)) << 32) | RESP_LEN;
    }
}
