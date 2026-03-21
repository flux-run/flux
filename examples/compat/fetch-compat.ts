// @ts-nocheck
// Compat test: fetch (native Web API) — exhaustive coverage
// Tests: GET, POST, PUT, DELETE, headers, redirects, streaming, timeouts, concurrency, errors
import { Hono } from "npm:hono";

const app = new Hono();

// ── Smoke ─────────────────────────────────────────────────────────────────

app.get("/", (c) => c.json({ library: "fetch", ok: true }));

// ── Happy path ────────────────────────────────────────────────────────────

// GET /get — basic GET, verifies response body and status
app.get("/get", async (c) => {
  const res = await fetch("https://httpbin.org/get?from=flux");
  const data = await res.json();
  return c.json({ ok: res.status === 200, origin_present: typeof data?.origin === "string" });
});

// POST /post — JSON body echoed back
app.post("/post", async (c) => {
  const body = await c.req.json();
  const res = await fetch("https://httpbin.org/post", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json();
  return c.json({ ok: res.status === 200, echoed: data?.json });
});

// PUT /put — PUT method support
app.put("/put", async (c) => {
  const body = await c.req.json();
  const res = await fetch("https://httpbin.org/put", {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json();
  return c.json({ ok: res.status === 200, echoed: data?.json });
});

// DELETE /delete — DELETE method support
app.delete("/delete", async (c) => {
  const res = await fetch("https://httpbin.org/delete", { method: "DELETE" });
  return c.json({ ok: res.status === 200 });
});

// PATCH /patch — PATCH method support
app.patch("/patch", async (c) => {
  const body = await c.req.json();
  const res = await fetch("https://httpbin.org/patch", {
    method: "PATCH",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json();
  return c.json({ ok: res.status === 200, echoed: data?.json });
});

// GET /headers — custom request headers are forwarded
app.get("/headers", async (c) => {
  const res = await fetch("https://httpbin.org/headers", {
    headers: { "x-flux-test": "hello", "x-custom-id": "42" },
  });
  const data = await res.json();
  return c.json({
    ok: true,
    has_flux_header: data?.headers?.["X-Flux-Test"] === "hello",
    has_custom_id: data?.headers?.["X-Custom-Id"] === "42",
  });
});

// GET /response-headers — reads custom response headers
app.get("/response-headers", async (c) => {
  const res = await fetch("https://httpbin.org/response-headers?x-flux-response=yes");
  const header = res.headers.get("x-flux-response") ?? res.headers.get("X-Flux-Response");
  return c.json({ ok: true, header_present: header === "yes" });
});

// GET /text — text/plain response
app.get("/text", async (c) => {
  const res = await fetch("https://httpbin.org/robots.txt");
  const text = await res.text();
  return c.json({ ok: res.status === 200, is_text: typeof text === "string", len: text.length });
});

// GET /binary — binary (image) response
app.get("/binary", async (c) => {
  const res = await fetch("https://httpbin.org/image/png");
  const buf = await res.arrayBuffer();
  return c.json({ ok: res.status === 200, bytes: buf.byteLength });
});

// GET /query — query string passthrough
app.get("/query", async (c) => {
  const res = await fetch("https://httpbin.org/get?foo=bar&baz=qux");
  const data = await res.json();
  return c.json({
    ok: res.status === 200,
    foo: data?.args?.foo === "bar",
    baz: data?.args?.baz === "qux",
  });
});

// POST /form — application/x-www-form-urlencoded body
app.post("/form", async (c) => {
  const res = await fetch("https://httpbin.org/post", {
    method: "POST",
    headers: { "content-type": "application/x-www-form-urlencoded" },
    body: "field1=hello&field2=world",
  });
  const data = await res.json();
  return c.json({
    ok: res.status === 200,
    field1: data?.form?.field1 === "hello",
  });
});

// GET /gzip — gzip-encoded response decompressed automatically
app.get("/gzip", async (c) => {
  const res = await fetch("https://httpbin.org/gzip");
  const data = await res.json();
  return c.json({ ok: data?.gzipped === true });
});

// GET /deflate — deflate-encoded response
app.get("/deflate", async (c) => {
  const res = await fetch("https://httpbin.org/deflate");
  const data = await res.json();
  return c.json({ ok: data?.deflated === true });
});

// ── Failure / edge cases ──────────────────────────────────────────────────

// GET /status-4xx — 4xx responses are returned, not thrown
app.get("/status-4xx", async (c) => {
  const res = await fetch("https://httpbin.org/status/404");
  return c.json({ ok: true, status: res.status, is_4xx: res.status === 404 });
});

// GET /status-5xx — 5xx responses are returned, not thrown
app.get("/status-5xx", async (c) => {
  const res = await fetch("https://httpbin.org/status/500");
  return c.json({ ok: true, status: res.status, is_5xx: res.status === 500 });
});

// GET /timeout — AbortSignal.timeout() cancels the request
app.get("/timeout", async (c) => {
  try {
    await fetch("https://httpbin.org/delay/30", { signal: AbortSignal.timeout(500) });
    return c.json({ ok: false, error: "expected timeout" }, 500);
  } catch (e) {
    return c.json({ ok: true, caught: true, name: e?.name });
  }
});

// GET /abort — explicit AbortController cancels the request
app.get("/abort", async (c) => {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), 300);
  try {
    await fetch("https://httpbin.org/delay/30", { signal: controller.signal });
    clearTimeout(timer);
    return c.json({ ok: false, error: "expected abort" }, 500);
  } catch (e) {
    clearTimeout(timer);
    return c.json({ ok: true, caught: true, aborted: true });
  }
});

// GET /unreachable — connection refused returns a network error
app.get("/unreachable", async (c) => {
  try {
    await fetch("http://0.0.0.0:19999/nope", { signal: AbortSignal.timeout(2000) });
    return c.json({ ok: false, error: "expected connection error" }, 500);
  } catch (e) {
    return c.json({ ok: true, caught: true, error_type: e?.name ?? String(e) });
  }
});

// GET /invalid-url — malformed URL throws synchronously or rejects
app.get("/invalid-url", async (c) => {
  try {
    await fetch("not-a-valid-url");
    return c.json({ ok: false }, 500);
  } catch (e) {
    return c.json({ ok: true, caught: true });
  }
});

// POST /large-body — send a large request body (>64KB)
app.post("/large-body", async (c) => {
  const largeStr = "x".repeat(100_000);
  const res = await fetch("https://httpbin.org/post", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ data: largeStr }),
  });
  return c.json({ ok: res.status === 200, sent_bytes: largeStr.length });
});

// GET /redirect — follows redirects transparently
app.get("/redirect", async (c) => {
  const res = await fetch("https://httpbin.org/redirect/2");
  return c.json({ ok: res.status === 200, final_url_present: !!res.url });
});

// GET /no-redirect — redirect NOT followed when redirect: "manual"
app.get("/no-redirect", async (c) => {
  const res = await fetch("https://httpbin.org/redirect/1", { redirect: "manual" });
  return c.json({ ok: true, status: res.status, redirected: res.redirected });
});

// GET /bearer-auth — Authorization header sent correctly
app.get("/bearer-auth", async (c) => {
  const res = await fetch("https://httpbin.org/bearer", {
    headers: { Authorization: "Bearer flux-test-token" },
  });
  const data = await res.json();
  return c.json({ ok: res.status === 200, authenticated: data?.authenticated === true });
});

// ── Concurrency ────────────────────────────────────────────────────────────

// GET /concurrent-3 — 3 fetches in parallel
app.get("/concurrent-3", async (c) => {
  const [r1, r2, r3] = await Promise.all([
    fetch("https://httpbin.org/get?req=1").then((r) => r.json()),
    fetch("https://httpbin.org/get?req=2").then((r) => r.json()),
    fetch("https://httpbin.org/get?req=3").then((r) => r.json()),
  ]);
  return c.json({
    ok: true,
    count: 3,
    all_have_origin: [r1, r2, r3].every((r) => typeof r?.origin === "string"),
  });
});

// GET /concurrent-mixed — concurrent GET + POST in parallel
app.get("/concurrent-mixed", async (c) => {
  const [get, post] = await Promise.all([
    fetch("https://httpbin.org/get").then((r) => r.json()),
    fetch("https://httpbin.org/post", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ tag: "concurrent" }),
    }).then((r) => r.json()),
  ]);
  return c.json({ ok: true, get_ok: !!get?.origin, post_ok: post?.json?.tag === "concurrent" });
});

// GET /sequential — 3 fetches in sequence, total results returned
app.get("/sequential", async (c) => {
  const results: number[] = [];
  for (let i = 1; i <= 3; i++) {
    const r = await fetch(`https://httpbin.org/get?seq=${i}`).then((r) => r.json());
    results.push(Number(r?.args?.seq));
  }
  return c.json({ ok: true, results });
});

// ── Aliases expected by the integration test runner ────────────────────────
// Runner uses descriptive prefixed names; the originals remain for backwards compat.

app.get("/fetch-external", async (c) => {
  const res = await fetch("https://httpbin.org/get?from=flux");
  const data = await res.json().catch(() => null);
  return c.json({ ok: res.status === 200, origin_present: typeof data?.origin === "string", status: res.status });
});

app.post("/fetch-post", async (c) => {
  const body = await c.req.json();
  const res = await fetch("https://httpbin.org/post", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  });
  const data = await res.json().catch(() => null);
  return c.json({ ok: res.status === 200, echoed: data?.json });
});

app.get("/fetch-headers", async (c) => {
  const res = await fetch("https://httpbin.org/headers", {
    headers: { "x-flux-test": "hello", "x-custom-id": "42" },
  });
  const data = await res.json().catch(() => null);
  return c.json({
    ok: true,
    has_custom_header: data?.headers?.["X-Flux-Test"] === "hello",
  });
});

app.get("/fetch-404", async (c) => {
  const res = await fetch("https://httpbin.org/status/404");
  return c.json({ ok: true, handled: true, upstream_status: res.status });
});

app.get("/fetch-500", async (c) => {
  const res = await fetch("https://httpbin.org/status/500");
  return c.json({ ok: true, upstream_status: res.status });
});

app.get("/fetch-refused", async (c) => {
  try {
    await fetch("http://127.0.0.1:19999/nope", { signal: AbortSignal.timeout(2000) });
    return c.json({ ok: false, caught: false });
  } catch (e) {
    return c.json({ ok: true, caught: true });
  }
});

app.get("/concurrent", async (c) => {
  const [r1, r2, r3] = await Promise.all([
    fetch("https://httpbin.org/get?req=1").then((r) => r.json()).catch(() => null),
    fetch("https://httpbin.org/get?req=2").then((r) => r.json()).catch(() => null),
    fetch("https://httpbin.org/get?req=3").then((r) => r.json()).catch(() => null),
  ]);
  return c.json({
    ok: true,
    count: 3,
    all_have_origin: [r1, r2, r3].every((r) => typeof r?.origin === "string"),
  });
});

Deno.serve(app.fetch);
