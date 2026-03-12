/*
 * hello_wasm_c — Fluxbase "hello world" function in C.
 *
 * Build:
 *   make          (requires wasi-sdk at /opt/wasi-sdk)
 *
 * Deploy:
 *   flux deploy   (runs make, then uploads handler.wasm)
 *
 * Invoke:
 *   flux invoke hello_wasm_c '{"name":"Alice"}'
 *   # → {"message":"Hello Alice!"}
 */

#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <stdio.h>

/* ── Fluxbase host imports ───────────────────────────────────────────────── */

__attribute__((import_module("fluxbase"), import_name("log")))
extern void __flux_log(int32_t level, const char *ptr, int32_t len);

/* ── ABI exports ─────────────────────────────────────────────────────────── */

__attribute__((export_name("__flux_alloc")))
void *flux_alloc(int32_t size) {
    return malloc((size_t)size);
}

static void log_str(const char *msg) {
    __flux_log(1, msg, (int32_t)strlen(msg));
}

static int32_t write_result(const char *json) {
    int32_t len = (int32_t)strlen(json);
    char   *buf = (char *)malloc(4 + (size_t)len);
    if (!buf) return 0;
    buf[0] = (char)( len        & 0xff);
    buf[1] = (char)((len >>  8) & 0xff);
    buf[2] = (char)((len >> 16) & 0xff);
    buf[3] = (char)((len >> 24) & 0xff);
    memcpy(buf + 4, json, (size_t)len);
    return (int32_t)(uintptr_t)buf;
}

/* ── Handler ─────────────────────────────────────────────────────────────── */

__attribute__((export_name("handle")))
int32_t handle(int32_t payload_ptr, int32_t payload_len) {
    const char *payload = (const char *)(uintptr_t)payload_ptr;
    log_str("hello_wasm_c: executing");

    /* Naive "name" field extraction */
    char name[128] = "world";
    const char *p  = strstr(payload, "\"name\"");
    if (p) {
        p = strchr(p + 6, ':');
        if (p) {
            p = strchr(p, '"');
            if (p) {
                p++;
                int i = 0;
                while (*p && *p != '"' && i < 127)
                    name[i++] = *p++;
                name[i] = '\0';
            }
        }
    }

    char result[256];
    snprintf(result, sizeof(result),
             "{\"output\":{\"message\":\"Hello %s!\"}}",
             name);
    return write_result(result);
}
