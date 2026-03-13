// hello — Flux function (compiled to WASM via clang++ --target=wasm32)
// Build: clang++ --target=wasm32 -nostdlib -Wl,--no-entry -Wl,--export-all -o hello.wasm handler.cpp
#include <cstdint>
#include <cstring>

static char output_buf[1024];

extern "C" {
    __attribute__((export_name("hello_handler")))
    uint64_t handler(uint8_t* input, uint32_t input_len) {
        (void)input; (void)input_len;

        const char* resp = "{\"ok\":true}";
        uint32_t resp_len = static_cast<uint32_t>(strlen(resp));
        memcpy(output_buf, resp, resp_len);

        return (static_cast<uint64_t>(reinterpret_cast<uintptr_t>(output_buf)) << 32) | resp_len;
    }
}
