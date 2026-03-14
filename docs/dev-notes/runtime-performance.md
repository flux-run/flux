# Runtime Performance Benchmarks

Measured on Apple M-series Mac, debug binary, Neon PostgreSQL (ap-southeast-1).  
All numbers are end-to-end HTTP round-trip including DB write where applicable.

---

## TypeScript / JavaScript (Deno V8 isolate pool)

**Date:** 2026-03-14  
**Binary:** `target/debug/server` (unoptimized — release build expected ~2–3× faster)  
**Isolate pool:** 16 workers, bundle-key affinity routing  
**Samples:** 20 warm sequential + 1 cold + 10-req concurrent burst  

| Function            | Runtime    | Cold start | Warm p50 | Warm p95 | 10-req concurrent wall |
|---------------------|------------|------------|----------|----------|------------------------|
| `hello`             | TypeScript | 12.3 ms    | 1.0 ms   | 1.5 ms   | 11.2 ms                |
| `greet`             | TypeScript | 2.4 ms     | 0.8 ms   | 1.2 ms   | 9.4 ms                 |
| `hello-javascript`  | JavaScript | 2.7 ms     | 0.8 ms   | 2.8 ms   | 4.0 ms                 |

### Notes

- **Warm p50 is sub-millisecond (0.8–1.0 ms)** — the V8 isolate pool is working correctly; a warmed isolate costs essentially nothing on re-entry.
- **Cold start** — `hello` (TS) was 12 ms on the very first call because it had to parse and compile the esbuild bundle from scratch. Subsequent first calls (`greet`, `hello-javascript`) were 2–3 ms as those bundles were simpler / the JIT was already warm.
- **JS vs TS** — warm performance is identical. JS concurrent burst was faster (4 ms vs 9–11 ms) because `greet`/`hello` perform a `ctx.db` write (Neon round-trip ~8 ms to ap-southeast-1) while `hello-javascript` returns `{ ok: true }` immediately without touching the DB.
- **Neon latency dominates warm wall time** for DB-touching functions. With a local Postgres the 10-req wall time would be ~1–2 ms.
- Release build (`cargo build --release`) should reduce cold start to ~3–5 ms and warm p50 to ~0.3–0.5 ms.

### Verdict

**Keep.** The Deno V8 engine is the primary JS/TS runtime. Numbers are already production-grade on a debug build — sub-millisecond warm latency with a 16-worker pool handles substantial concurrency before any queuing.

---

## WASM Runtimes (Wasmtime + Cranelift AOT)

**Date:** 2026-03-14  
**Binary:** `target/debug/server` (unoptimized)  
**WASM pool:** 16 workers, 10 billion fuel units, 120 s timeout  
**Samples:** 20 warm sequential after one cold invocation  
**Payload:** `{}` (no DB or queue I/O)

### Cold start

Two cold-start modes exist depending on whether the disk cache is populated:

**First-ever call on a fresh machine (Cranelift AOT compilation):**  
Module compilation is a one-time cost. The `.cwasm` result is saved to `~/.flux/wasm-cache/` and reused on all subsequent server restarts.

| Function              | Language        | WASM size | Cold start (Cranelift, first ever) |
|-----------------------|-----------------|-----------|-------------------------------------|
| `hello-rust`          | Rust            | ~100 KB   | ~1.1 s                              |
| `hello-assemblyscript`| AssemblyScript  | ~50 KB    | ~0.5 s                              |
| `hello-go`            | Go (wasip1)     | 3.1 MB    | ~27 s                               |
| `hello-java`          | Java (TeaVM)    | ~500 KB   | ~1.7 s                              |
| `hello-python`        | Python (py2wasm)| ~26 MB    | ~62 s                               |
| `hello-php`           | PHP (php-8.2-wasm) | 13 MB  | ~88 s (OptLevel::None engine)       |
| `hello-ruby`          | Ruby (rbwasm)   | ~47 MB    | > 120 s (timeout)                   |

**Subsequent server restarts (disk AOT cache hit — `Module::deserialize_file`):**  
The pre-compiled `.cwasm` artifact is loaded from `~/.flux/wasm-cache/` keyed by FNV-1a content hash. This replaces Cranelift compilation entirely.

| Function              | Language        | WASM size | .cwasm size | Cold start (disk cache) |
|-----------------------|-----------------|-----------|-------------|-------------------------|
| `hello-rust`          | Rust            | ~100 KB   | < 1 MB      | < 10 ms                 |
| `hello-assemblyscript`| AssemblyScript  | ~50 KB    | < 1 MB      | < 10 ms                 |
| `hello-java`          | Java (TeaVM)    | ~500 KB   | ~3 MB       | ~15 ms                  |
| `hello-go`            | Go (wasip1)     | 3.1 MB    | ~15 MB      | ~100 ms (estimated)     |
| `hello-php`           | PHP 8.2 WASM    | 13 MB     | ~63 MB      | ~800 ms (measured)      |
| `hello-python`        | Python          | ~26 MB    | ~100 MB     | ~2 s (estimated)        |
| `hello-ruby`          | Ruby            | ~47 MB    | —           | never compiled (timeout) |

### Warm latency (module compiled and in LRU cache)

| Function              | Language       | Warm p50 | Warm p95 |
|-----------------------|----------------|----------|----------|
| `hello-rust`          | Rust           | 2.5 ms   | 2.7 ms   |
| `hello-assemblyscript`| AssemblyScript | 2.1 ms   | 16.9 ms  |
| `hello-go`            | Go (wasip1)    | 20.0 ms  | 22.3 ms  |
| `hello-java`          | Java (TeaVM)   | 3.2 ms   | 3.3 ms   |
| `hello-python`        | Python         | 181 ms   | 185 ms   |
| `hello-php`           | PHP 8.2 WASM   | 84 ms    | 85 ms    |
| `hello-ruby`          | Ruby           | — (times out) | — |

For comparison — **JS/TS (V8)**: warm p50 **0.8–1.0 ms**, cold start **2–12 ms**.

### Notes

**poll_oneoff fix (2026-03-14):** Go's `wasip1` goroutine scheduler calls `poll_oneoff`
before every `fd_read` to check stdin readiness. The original WASI stub returned 0 events
unconditionally, causing Go to busy-spin through 10 billion fuel units (~120 s) before
timing out. Fixed by implementing proper event dispatch: FD_READ subscriptions on fd=0
report `ready` when `stdin_buf` has unread bytes; CLOCK subscriptions always fire
immediately (no wall-clock needed); a synthetic CLOCK event is always returned as a
fallback to prevent spin-wait after stdin EOF.

**Go (wasip1) — 27 s cold start:** The Go runtime compiles its entire standard library
into a single 3.1 MB WASM binary. Cranelift AOT-compiles all of it on first use. Warm
execution is 20 ms (expensive goroutine scheduler + GC startup on every invocation — Go
re-initialises its runtime per `_start` call). Go WASM is functional but the per-call
overhead makes it uncompetitive with hand-compiled Rust/AssemblyScript or the V8 isolate
pool.

**Rust / AssemblyScript — best WASM targets:** 2–3 ms warm latency, small binaries,
fast cold starts. Rust (custom Flux ABI via `__flux_alloc + handle`) and AssemblyScript
(same ABI) are the recommended WASM languages.

**Java (TeaVM) — surprisingly good:** 3.2 ms warm, 1.7 s cold. TeaVM produces compact
WASM and doesn't carry a JVM runtime. Good candidate for teams that prefer Java/Kotlin.

**Python / Ruby — impractical:** Both compile their entire interpreter into WASM (26 MB
and 47 MB respectively). Python takes 62 s to compile on first use and 181 ms per warm
call (interpreter overhead). Ruby exceeds the 120 s compilation timeout. These runtimes
are not viable in a serverless context without pre-compiled module caching at deploy time.

**PHP (php-8.2-wasm) — functional with AOT disk cache:** PHP ships as a 13 MB WASM
binary (`php-8.2.6-wasmedge.wasm` from vmware-labs/webassembly-language-runtimes, despite
the name it uses only `wasi_snapshot_preview1` and works on standard Wasmtime). The PHP
interpreter binary contains a 207 KB dispatch function that causes Cranelift's optimizer
to run for ~88 s on first compile. We added a second "fast engine" (`OptLevel::None`) that
kicks in for WASM binaries >5 MB, bringing first-compile time down from effectively
infinite to 88 s. The resulting `.cwasm` artifact (63 MB of ARM64 native code) is saved
to `~/.flux/wasm-cache/` and reloaded in ~800 ms on subsequent server restarts. Warm
execution is 84 ms — PHP interpreter startup dominates per-call. This is actually faster
than Python warm (181 ms) because PHP's CLI interpreter is leaner, but both are far slower
than compiled targets (Rust: 2.5 ms).

The CLI call uses `flux.wasi-args` custom WASM section (NUL-separated argv bytes) to
embed `["php", "-r", "<script>"]` into the binary at build time, so no argv is passed
at runtime — the worker reads it from the section.

**AOT disk cache (2026-03-14):** All WASM modules are now serialized to
`~/.flux/wasm-cache/<fnv1a-hash>-<engine>.cwasm` after first Cranelift compile. On
subsequent server restarts the pre-compiled artifact is deserialized via
`Module::deserialize_file` (unsafe — Wasmtime validates architecture + version
compatibility). Cache key is FNV-1a 64-bit hash of the raw WASM bytes, which is stable
across process restarts (unlike `DefaultHasher` which uses a random seed per process).

### Verdict

| Runtime        | Status       | Notes |
|----------------|--------------|-------|
| TypeScript/JS  | ✅ Primary   | Sub-ms warm, 16-worker V8 pool |
| Rust (WASM)    | ✅ Keep      | 2.5 ms warm, ideal for compute-heavy tasks |
| AssemblyScript | ✅ Keep      | 2.1 ms warm, easy JS-like syntax |
| Java (TeaVM)   | ✅ Keep      | 3.2 ms warm, good Java/Kotlin story |
| Go (wasip1)    | ⚠️ Limited   | 20 ms warm, 27 s cold — functional but slow |
| PHP (php-8.2)  | ⚠️ Limited   | 84 ms warm, ~800 ms cold (disk cache) / 88 s (first ever); socket stubs ENOSYS |
| Python         | ❌ Not viable | 181 ms warm, 62 s cold — interpreter overhead |
| Ruby           | ❌ Not viable | Compilation timeout (>120 s) |

---

## Test Setup

```bash
# Start server (Neon DB)
LOCAL_MODE=true INTERNAL_SERVICE_TOKEN=dev-token-123 \
  DATABASE_URL="<neon-url>" SQLX_OFFLINE=true \
  ./target/debug/server

# Deploy test functions
cd /tmp/flux-test-app && flux deploy --force

# Run benchmark
python3 docs/dev-notes/bench_ts_js.py
```

Invoke path used: `POST http://localhost:4000/flux/dev/invoke/{name}`  
Payload: `{"name": "world"}`
