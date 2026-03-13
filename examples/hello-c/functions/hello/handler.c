// hello — Flux function (compiled to WASM via clang --target=wasm32)
// Build: clang --target=wasm32 -nostdlib -Wl,--no-entry -Wl,--export-all -o hello.wasm handler.c
#include <stdint.h>
#include <string.h>

// Simple JSON response helper — replace with a proper JSON library.
static char output_buf[1024];

__attribute__((export_name("hello_handler")))
uint64_t handler(uint8_t *input, uint32_t input_len) {
    (void)input; (void)input_len;

    const char *resp = "{\"ok\":true}";
    uint32_t resp_len = (uint32_t)strlen(resp);
    memcpy(output_buf, resp, resp_len);

    return ((uint64_t)(uintptr_t)output_buf << 32) | resp_len;
}
