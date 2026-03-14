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

**Date:** 2026-03-15  
**Binary:** `target/debug/server` (unoptimized) via `flux dev` (embedded PostgreSQL)  
**WASM pool:** 16 workers, 10 billion fuel units, 120 s timeout  
**Samples:** 10 warm calls, p50/p95 from server-reported `duration_ms`  
**Payload:** `{}` (no DB or queue I/O)

### Cold start

Two cold-start modes exist depending on whether the disk cache is populated:

**First-ever call on a fresh machine (Cranelift AOT compilation):**  
Module compilation is a one-time cost. The `.cwasm` result is saved to `~/.flux/wasm-cache/` and reused on all subsequent server restarts.

| Function              | Language           | WASM size | .cwasm size | Cold start (Cranelift, first ever) |
|-----------------------|--------------------|-----------|-------------|-------------------------------------|
| `hello-assemblyscript`| AssemblyScript     | ~1.3 KB   | 66 KB       | **100 ms**                          |
| `hello-rust`          | Rust               | ~85 KB    | 344 KB      | **659 ms**                          |
| `hello-java`          | Java (TeaVM)       | ~192 KB   | 822 KB      | **1,427 ms**                        |
| `hello-go`            | Go (wasip1)        | 3.1 MB    | 10.7 MB     | **26,870 ms** (~27 s)               |
| `hello-python`        | Python (py2wasm)   | ~26 MB    | 28.3 MB     | **40,235 ms** (~40 s)               |
| `hello-php`           | PHP (php-8.2-wasm) | 13 MB     | 62.5 MB     | **89,693 ms** (~90 s, OptLevel::None engine) |
| `hello-ruby`          | Ruby (rbwasm)      | ~47 MB    | —           | > 200 s (timeout — never completes) |

**Subsequent server restarts (disk AOT cache hit — `Module::deserialize_file`):**  
The pre-compiled `.cwasm` artifact is loaded from `~/.flux/wasm-cache/` keyed by FNV-1a content hash. This replaces Cranelift compilation entirely.

| Function              | Language           | WASM size | .cwasm size | Cold start (disk cache) |
|-----------------------|--------------------|-----------|-------------|-------------------------|
| `hello-assemblyscript`| AssemblyScript     | ~1.3 KB   | 66 KB       | **3 ms**                |
| `hello-rust`          | Rust               | ~85 KB    | 344 KB      | **3 ms**                |
| `hello-java`          | Java (TeaVM)       | ~192 KB   | 822 KB      | **5 ms**                |
| `hello-go`            | Go (wasip1)        | 3.1 MB    | 10.7 MB     | **57 ms**               |
| `hello-python`        | Python (py2wasm)   | ~26 MB    | 28.3 MB     | **480 ms**              |
| `hello-php`           | PHP (php-8.2-wasm) | 13 MB     | 62.5 MB     | **256 ms**              |
| `hello-ruby`          | Ruby (rbwasm)      | ~47 MB    | —           | — (cwasm never produced) |

### Warm latency (module compiled and in LRU cache)

Server-side execution time (`duration_ms` field), 10 calls each.

| Function              | Language       | Warm p50 | Warm p95 |
|-----------------------|----------------|----------|----------|
| `hello-assemblyscript`| AssemblyScript | < 1 ms   | 1 ms     |
| `hello-rust`          | Rust           | 1 ms     | 2 ms     |
| `hello-java`          | Java (TeaVM)   | 1 ms     | 4 ms     |
| `hello-go`            | Go (wasip1)    | 19 ms    | 54 ms    |
| `hello-php`           | PHP 8.2 WASM   | 83 ms    | 84 ms    |
| `hello-python`        | Python         | 191 ms   | 194 ms   |
| `hello-ruby`          | Ruby           | — (times out) | —   |

For comparison — **JS/TS (V8)**: warm p50 **0.8–1.0 ms**, cold start **2–12 ms**.

### Notes

**sock_accept signature compatibility fix (2026-03-15):** PHP (WasmEdge variant) uses a
non-standard 2-param `sock_accept(fd, addr_out) -> i32`, while Python (standard WASI) and
most other runtimes use the 3-param form `sock_accept(fd, flags, result_fd) -> i32`. The
WASM executor now inspects each module's actual import type via `module.imports()` and
registers the matching `ENOSYS` stub dynamically using `linker.func_new()` with the exact
`FuncType`. This keeps both PHP and Python working from the same binary with a single
shared linker — no per-runtime branching needed.

**poll_oneoff fix (2026-03-14):** Go's `wasip1` goroutine scheduler calls `poll_oneoff`
before every `fd_read` to check stdin readiness. The original WASI stub returned 0 events
unconditionally, causing Go to busy-spin through 10 billion fuel units (~120 s) before
timing out. Fixed by implementing proper event dispatch: FD_READ subscriptions on fd=0
report `ready` when `stdin_buf` has unread bytes; CLOCK subscriptions always fire
immediately (no wall-clock needed); a synthetic CLOCK event is always returned as a
fallback to prevent spin-wait after stdin EOF.

**Go (wasip1) — 27 s cold start:** The Go runtime compiles its entire standard library
into a single 3.1 MB WASM binary. Cranelift AOT-compiles all of it on first use. Warm
execution is 19 ms (expensive goroutine scheduler + GC startup on every invocation — Go
re-initialises its runtime per `_start` call). The 10.7 MB `.cwasm` loads in 57 ms on
restart. Go WASM is functional but the per-call overhead makes it uncompetitive with
hand-compiled Rust/AssemblyScript or the V8 isolate pool.

**Rust / AssemblyScript — best WASM targets:** Sub-millisecond warm latency, tiny binaries
(< 344 KB `.cwasm`), 3 ms disk-cache cold start. Rust (custom Flux ABI via
`__flux_alloc + handle`) and AssemblyScript (same ABI) are the recommended WASM languages.

**Java (TeaVM) — surprisingly good:** 1 ms warm, 1.4 s cold, 5 ms disk-cache cold start.
TeaVM produces compact WASM and doesn't carry a JVM runtime. Good candidate for teams that
prefer Java/Kotlin.

**Python / Ruby — impractical:** Both compile their entire interpreter into WASM (26 MB
and 47 MB respectively). Python takes ~40 s to compile on first use (OptLevel::None fast
engine — full optimizer would be longer), and 191 ms per warm call (interpreter overhead).
The 28.3 MB `.cwasm` loads in 480 ms on restart. Ruby exceeds the 200 s compilation
timeout even with `OptLevel::None`. These runtimes are not viable in a serverless context
without pre-compiled module caching at deploy time.

**PHP (php-8.2-wasm) — functional with AOT disk cache:** PHP ships as a 13 MB WASM
binary (`php-8.2.6-wasmedge.wasm` from vmware-labs/webassembly-language-runtimes, despite
the name it uses only `wasi_snapshot_preview1` and works on standard Wasmtime). The PHP
interpreter binary contains a 207 KB dispatch function that causes Cranelift's optimizer
to run for ~90 s on first compile. A second "fast engine" (`OptLevel::None`) kicks in for
WASM binaries >5 MB, keeping compilation to ~90 s. The resulting 62.5 MB `.cwasm`
artifact is saved to `~/.flux/wasm-cache/` and reloaded in **256 ms** on subsequent
server restarts. Warm execution is 83 ms — PHP interpreter startup dominates per-call.

The CLI call uses `flux.wasi-args` custom WASM section (NUL-separated argv bytes) to
embed `["php", "-r", "<script>"]` into the binary at build time, so no argv is passed
at runtime — the worker reads it from the section.

**AOT disk cache (2026-03-14):** All WASM modules are serialized to
`~/.flux/wasm-cache/<fnv1a-hash>-<engine>.cwasm` after first Cranelift compile. On
subsequent server restarts the pre-compiled artifact is deserialized via
`Module::deserialize_file` (unsafe — Wasmtime validates architecture + version
compatibility). Cache key is FNV-1a 64-bit hash of the raw WASM bytes, which is stable
across process restarts (unlike `DefaultHasher` which uses a random seed per process).

### Verdict

| Runtime        | Status        | Warm p50 | Disk-cache cold | Cranelift cold |
|----------------|---------------|----------|-----------------|----------------|
| TypeScript/JS  | ✅ Primary    | < 1 ms   | 2–12 ms         | 2–12 ms        |
| AssemblyScript | ✅ Keep       | < 1 ms   | 3 ms            | 100 ms         |
| Rust (WASM)    | ✅ Keep       | 1 ms     | 3 ms            | 659 ms         |
| Java (TeaVM)   | ✅ Keep       | 1 ms     | 5 ms            | 1.4 s          |
| Go (wasip1)    | ⚠️ Limited    | 19 ms    | 57 ms           | 27 s           |
| PHP (php-8.2)  | ⚠️ Limited    | 83 ms    | 256 ms          | 90 s           |
| Python         | ❌ Not viable | 191 ms   | 480 ms          | 40 s           |
| Ruby           | ❌ Not viable | —        | —               | > 200 s        |

---

## Test Setup

```bash
# Start server with embedded PostgreSQL
cd /tmp/flux-test-app && flux dev

# Deploy test functions (from project root)
flux deploy

# Run benchmark
python3 /tmp/bench_all.py
```

Invoke path used: `POST http://localhost:4000/flux/dev/invoke/{name}`  
Payload: `{"name": "world"}`
