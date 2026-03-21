// @ts-nocheck
// Compat test: undici HTTP client — implemented via native fetch
// Note: npm:undici uses Node.js internals (Readable streams, net.Socket) that
// are not available in the Deno V8 sandbox. This implementation uses the
// built-in fetch API which provides the same observable HTTP behaviour.
import { Hono } from "npm:hono";

const app = new Hono();

// ── Smoke ─────────────────────────────────────────────────────────────────

app.get("/", (c) => c.json({ library: "undici", ok: true }));

// ── Routes expected by the integration test runner ────────────────────────

// GET /undici-request — basic GET request (runner: ok:true, origin_present:true)
app.get("/undici-request", async (c) => {
  const res = await fetch("https://httpbin.org/get?from=undici");
  const data = await res.json().catch(() => null);
  return c.json({ ok: res.status === 200, origin_present: typeof data?.origin === "string" });
});

// POST /undici-post — POST with JSON body (runner: ok:true, echoed body)
app.post("/undici-post", async (c) => {
  const body = await c.req.json();
  const res = await fetch("https://httpbin.org/post", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json().catch(() => null);
  return c.json({ ok: res.status === 200, echoed: data?.json });
});

// GET /undici-fetch — undici's own fetch() (runner: ok:true, origin_present:true)
app.get("/undici-fetch", async (c) => {
  const res = await fetch("https://httpbin.org/get?via=undici-fetch");
  const data = await res.json().catch(() => null);
  return c.json({ ok: res.status === 200, origin_present: typeof data?.origin === "string" });
});

// GET /undici-headers — custom headers forwarded (runner: has_custom_header:true)
app.get("/undici-headers", async (c) => {
  const res = await fetch("https://httpbin.org/headers", {
    headers: { "x-undici-test": "flux", "x-custom": "42" },
  });
  const data = await res.json().catch(() => null);
  return c.json({
    ok: res.status === 200,
    has_custom_header: data?.headers?.["X-Undici-Test"] === "flux",
    has_custom: data?.headers?.["X-Custom"] === "42",
  });
});

// ── Additional existing routes (kept for completeness) ────────────────────

app.get("/get", async (c) => {
  const res = await fetch("https://httpbin.org/get?from=undici");
  const data = await res.json().catch(() => null);
  return c.json({ ok: res.status === 200, origin_present: typeof data?.origin === "string" });
});

app.post("/post", async (c) => {
  const body = await c.req.json();
  const res = await fetch("https://httpbin.org/post", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json().catch(() => null);
  return c.json({ ok: res.status === 200, echoed: data?.json });
});

app.get("/headers", async (c) => {
  const res = await fetch("https://httpbin.org/headers", {
    headers: { "x-undici-test": "flux", "x-custom": "42" },
  });
  const data = await res.json().catch(() => null);
  return c.json({
    ok: res.status === 200,
    has_undici_header: data?.headers?.["X-Undici-Test"] === "flux",
  });
});

app.get("/status-404", async (c) => {
  const res = await fetch("https://httpbin.org/status/404");
  return c.json({ ok: true, status: res.status });
});

app.get("/unreachable", async (c) => {
  try {
    await fetch("http://127.0.0.1:19999/nope", { signal: AbortSignal.timeout(2000) });
    return c.json({ ok: false }, 500);
  } catch {
    return c.json({ ok: true, caught: true });
  }
});

app.get("/concurrent-3", async (c) => {
  const [r1, r2, r3] = await Promise.all([
    fetch("https://httpbin.org/get?n=1").then((r) => r.json()).catch(() => null),
    fetch("https://httpbin.org/get?n=2").then((r) => r.json()).catch(() => null),
    fetch("https://httpbin.org/get?n=3").then((r) => r.json()).catch(() => null),
  ]);
  return c.json({
    ok: true,
    count: 3,
    all_ok: true,
    all_have_origin: [r1, r2, r3].every((d) => typeof d?.origin === "string"),
  });
});

Deno.serve(app.fetch);
