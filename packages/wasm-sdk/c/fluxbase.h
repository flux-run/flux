/**
 * fluxbase.h — Fluxbase WASM SDK for C / C++
 *
 * Include this header in your handler translation unit.
 * Compile with wasi-sdk:
 *
 *   /opt/wasi-sdk/bin/clang \
 *       --sysroot=/opt/wasi-sdk/share/wasi-sysroot \
 *       -O2 \
 *       -o handler.wasm \
 *       handler.c
 *
 * ## ABI
 * The runtime calls `handle(payload_ptr, payload_len) -> result_ptr`.
 * Write the result as `[4-byte u32 LE length][UTF-8 JSON]` and return
 * the pointer to the start of that buffer.
 */
#pragma once

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── Host imports ─────────────────────────────────────────────────────────── */

/**
 * Emit a log line.
 * @param level  1=info, 2=warn, 3=error
 * @param ptr    Pointer to UTF-8 message bytes (not NUL-terminated).
 * @param len    Byte length of the message.
 */
__attribute__((import_module("fluxbase"), import_name("log")))
extern void flux_log(int32_t level, const char *ptr, int32_t len);

/**
 * Retrieve a secret by key.
 * @return Number of bytes written to `out_ptr`, or -1 if not found.
 */
__attribute__((import_module("fluxbase"), import_name("secrets_get")))
extern int32_t flux_secrets_get(
    const char *key_ptr, int32_t key_len,
    char       *out_ptr, int32_t out_max
);

/**
 * Perform an outbound HTTP request.
 *
 * `req_ptr` must point to a JSON string:
 *   {"method":"GET","url":"https://...","headers":{},"body":"<base64>"}
 *
 * On success writes the response JSON to `out_ptr` and returns the byte count.
 * On error returns a negative value.
 */
__attribute__((import_module("fluxbase"), import_name("http_fetch")))
extern int32_t flux_http_fetch(
    const char *req_ptr, int32_t req_len,
    char       *out_ptr, int32_t out_max
);

/* ── Required ABI export ──────────────────────────────────────────────────── */

/** Allocate `size` bytes from the heap (used by the host to push data in). */
__attribute__((export_name("__flux_alloc")))
static inline void *flux_alloc(int32_t size) {
    return malloc((size_t)size);
}

/* ── Convenience helpers ──────────────────────────────────────────────────── */

/** Log an info message (NUL-terminated C string). */
static inline void flux_log_str(const char *msg) {
    flux_log(1, msg, (int32_t)strlen(msg));
}

/** Log a warning (NUL-terminated C string). */
static inline void flux_warn_str(const char *msg) {
    flux_log(2, msg, (int32_t)strlen(msg));
}

/** Log an error (NUL-terminated C string). */
static inline void flux_error_str(const char *msg) {
    flux_log(3, msg, (int32_t)strlen(msg));
}

/**
 * Write a result buffer in the `[4-byte LE len][json]` layout and return
 * the pointer.  The caller is responsible for keeping the data alive until
 * the host has processed it (i.e. do not free it inside handle()).
 *
 * @param json  NUL-terminated JSON string.
 * @return Pointer to the encoded result buffer, or NULL on allocation failure.
 */
static inline int32_t flux_write_result(const char *json) {
    int32_t len = (int32_t)strlen(json);
    char   *buf = (char *)malloc(4 + (size_t)len);
    if (!buf) return 0;
    /* little-endian u32 length prefix */
    buf[0] = (char)( len        & 0xff);
    buf[1] = (char)((len >>  8) & 0xff);
    buf[2] = (char)((len >> 16) & 0xff);
    buf[3] = (char)((len >> 24) & 0xff);
    memcpy(buf + 4, json, (size_t)len);
    return (int32_t)(uintptr_t)buf;
}

/* ── Handler signature ────────────────────────────────────────────────────── */

/**
 * Implement this function in your handler.c:
 *
 *   int32_t handle(int32_t payload_ptr, int32_t payload_len) {
 *       const char *payload = (const char *)(uintptr_t)payload_ptr;
 *       flux_log_str("my-function: executing");
 *       return flux_write_result("{\"output\":{\"message\":\"hello\"}}");
 *   }
 */

#ifdef __cplusplus
} /* extern "C" */
#endif
