// @ts-nocheck
// Compat test: native fetch + JSON response parsing
import { Hono } from "npm:hono";

const app = new Hono();

// GET / — smoke test
app.get("/", (c) => c.json({ library: "fetch", ok: true }));

// GET /fetch-external — outbound GET (IO interception)
app.get("/fetch-external", async (c) => {
  const res = await fetch("https://httpbin.org/get?from=flux");
  const data = await res.json();
  return c.json({
    ok: true,
    status: res.status,
    origin_present: typeof data?.origin === "string",
  });
});

// POST /fetch-post — outbound POST
app.post("/fetch-post", async (c) => {
  const body = await c.req.json();
  const res = await fetch("https://httpbin.org/post", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json();
  return c.json({ ok: res.status === 200, echoed: data?.json });
});

// GET /fetch-headers — custom headers forwarding
app.get("/fetch-headers", async (c) => {
  const res = await fetch("https://httpbin.org/headers", {
    headers: { "x-flux-test": "hello" },
  });
  const data = await res.json();
  return c.json({
    ok: true,
    has_custom_header: typeof data?.headers?.["X-Flux-Test"] === "string",
  });
});

// ── Failure cases ──────────────────────────────────────────────────────────

// GET /fetch-404 — upstream returns 404, handled gracefully
app.get("/fetch-404", async (c) => {
  const res = await fetch("https://httpbin.org/status/404");
  return c.json({ ok: true, upstream_status: res.status, handled: res.status === 404 });
});

// GET /fetch-500 — upstream returns 500, handled gracefully
app.get("/fetch-500", async (c) => {
  const res = await fetch("https://httpbin.org/status/500");
  return c.json({ ok: true, upstream_status: res.status, handled: res.status === 500 });
});

// GET /fetch-refused — connection to an unreachable host (should reject gracefully)
app.get("/fetch-refused", async (c) => {
  try {
    await fetch("http://0.0.0.0:19999/nope", { signal: AbortSignal.timeout(2000) });
    return c.json({ ok: false, error: "expected error but none thrown" }, 500);
  } catch (e) {
    return c.json({ ok: true, caught: true, error_type: e?.name ?? String(e) });
  }
});

// ── Concurrency ────────────────────────────────────────────────────────────

// GET /concurrent — fires 3 outbound fetches in parallel via Promise.all
app.get("/concurrent", async (c) => {
  const [r1, r2, r3] = await Promise.all([
    fetch("https://httpbin.org/get?req=1").then((r) => r.json()),
    fetch("https://httpbin.org/get?req=2").then((r) => r.json()),
    fetch("https://httpbin.org/get?req=3").then((r) => r.json()),
  ]);
  return c.json({
    ok: true,
    count: 3,
    all_have_origin:
      typeof r1?.origin === "string" &&
      typeof r2?.origin === "string" &&
      typeof r3?.origin === "string",
  });
});

Deno.serve(app.fetch);
