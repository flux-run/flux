// @ts-nocheck
// Compat test: undici HTTP client — exhaustive coverage
// Tests: GET, POST, methods, headers, streams, errors, pipeline, timing, concurrency
import { Hono } from "npm:hono";
import { request, stream, pipeline, fetch as undiciFetch } from "npm:undici";

const app = new Hono();

// ── Smoke ─────────────────────────────────────────────────────────────────

app.get("/", (c) => c.json({ library: "undici", ok: true }));

// ── Happy path ────────────────────────────────────────────────────────────

// GET /get — basic GET via undici.request
app.get("/get", async (c) => {
  const { statusCode, body } = await request("https://httpbin.org/get?from=undici");
  const data = await body.json();
  return c.json({ ok: statusCode === 200, origin_present: typeof data?.origin === "string" });
});

// POST /post — POST with JSON body
app.post("/post", async (c) => {
  const reqBody = await c.req.json();
  const { statusCode, body } = await request("https://httpbin.org/post", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(reqBody),
  });
  const data = await body.json();
  return c.json({ ok: statusCode === 200, echoed: data?.json });
});

// PUT /put — PUT method
app.put("/put", async (c) => {
  const reqBody = await c.req.json();
  const { statusCode, body } = await request("https://httpbin.org/put", {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(reqBody),
  });
  const data = await body.json();
  return c.json({ ok: statusCode === 200, echoed: data?.json });
});

// DELETE /delete — DELETE method
app.delete("/delete", async (c) => {
  const { statusCode, body } = await request("https://httpbin.org/delete", { method: "DELETE" });
  await body.dump(); // consume body
  return c.json({ ok: statusCode === 200 });
});

// PATCH /patch — PATCH method
app.patch("/patch", async (c) => {
  const reqBody = await c.req.json();
  const { statusCode, body } = await request("https://httpbin.org/patch", {
    method: "PATCH",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(reqBody),
  });
  const data = await body.json();
  return c.json({ ok: statusCode === 200, echoed: data?.json });
});

// GET /headers — custom headers forwarded
app.get("/headers", async (c) => {
  const { statusCode, body } = await request("https://httpbin.org/headers", {
    headers: { "x-undici-test": "flux", "x-custom": "42" },
  });
  const data = await body.json();
  return c.json({
    ok: statusCode === 200,
    has_undici_header: data?.headers?.["X-Undici-Test"] === "flux",
    has_custom: data?.headers?.["X-Custom"] === "42",
  });
});

// GET /response-headers — read response headers from undici
app.get("/response-headers", async (c) => {
  const { headers, body } = await request(
    "https://httpbin.org/response-headers?x-undici-resp=yes",
  );
  await body.dump();
  const header = Array.isArray(headers?.["x-undici-resp"])
    ? headers["x-undici-resp"][0]
    : headers?.["x-undici-resp"];
  return c.json({ ok: true, header });
});

// GET /text — text response body via body.text()
app.get("/text", async (c) => {
  const { statusCode, body } = await request("https://httpbin.org/robots.txt");
  const text = await body.text();
  return c.json({ ok: statusCode === 200, is_text: typeof text === "string", len: text.length });
});

// GET /binary — binary response via body.arrayBuffer()
app.get("/binary", async (c) => {
  const { statusCode, body } = await request("https://httpbin.org/image/png");
  const buf = await body.arrayBuffer();
  return c.json({ ok: statusCode === 200, bytes: buf.byteLength });
});

// GET /gzip — gzip decompression
app.get("/gzip", async (c) => {
  const { statusCode, body } = await request("https://httpbin.org/gzip");
  // undici handles decompression internally
  let data: any;
  try {
    data = await body.json();
  } catch {
    const text = await body.text().catch(() => "");
    data = {};
  }
  return c.json({ ok: statusCode === 200, gzipped: data?.gzipped === true });
});

// GET /fetch-api — undici's fetch() compatible with native fetch
app.get("/fetch-api", async (c) => {
  const res = await undiciFetch("https://httpbin.org/get?via=undici-fetch");
  const data = await res.json();
  return c.json({ ok: res.status === 200, origin_present: typeof data?.origin === "string" });
});

// ── Failure / edge cases ──────────────────────────────────────────────────

// GET /status-404 — 404 does not throw; statusCode returned
app.get("/status-404", async (c) => {
  const { statusCode, body } = await request("https://httpbin.org/status/404");
  await body.dump();
  return c.json({ ok: true, status: statusCode });
});

// GET /status-500 — 500 does not throw
app.get("/status-500", async (c) => {
  const { statusCode, body } = await request("https://httpbin.org/status/500");
  await body.dump();
  return c.json({ ok: true, status: statusCode });
});

// GET /timeout — connect/read timeout
app.get("/timeout", async (c) => {
  try {
    const { body } = await request("https://httpbin.org/delay/30", {
      headersTimeout: 800,
      bodyTimeout: 800,
    });
    await body.dump();
    return c.json({ ok: false, error: "expected timeout" }, 500);
  } catch (e) {
    return c.json({ ok: true, caught: true, name: e?.name ?? String(e) });
  }
});

// GET /unreachable — connection refused
app.get("/unreachable", async (c) => {
  try {
    const { body } = await request("http://0.0.0.0:19999/nope", { connectTimeout: 1000 });
    await body.dump();
    return c.json({ ok: false }, 500);
  } catch (e) {
    return c.json({ ok: true, caught: true });
  }
});

// POST /large-body — large payload via undici
app.post("/large-body", async (c) => {
  const large = "x".repeat(100_000);
  const { statusCode, body } = await request("https://httpbin.org/post", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ data: large }),
  });
  await body.dump();
  return c.json({ ok: statusCode === 200, sent_bytes: large.length });
});

// ── Concurrency ────────────────────────────────────────────────────────────

// GET /concurrent-3 — 3 parallel undici requests
app.get("/concurrent-3", async (c) => {
  const [r1, r2, r3] = await Promise.all([
    request("https://httpbin.org/get?n=1"),
    request("https://httpbin.org/get?n=2"),
    request("https://httpbin.org/get?n=3"),
  ]);
  const [d1, d2, d3] = await Promise.all([r1.body.json(), r2.body.json(), r3.body.json()]);
  return c.json({
    ok: true,
    count: 3,
    all_ok: [r1, r2, r3].every((r) => r.statusCode === 200),
    all_have_origin: [d1, d2, d3].every((d) => typeof d?.origin === "string"),
  });
});

// GET /concurrent-mixed — GET + POST in parallel
app.get("/concurrent-mixed", async (c) => {
  const [get, post] = await Promise.all([
    request("https://httpbin.org/get"),
    request("https://httpbin.org/post", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ tag: "undici-concurrent" }),
    }),
  ]);
  const [getData, postData] = await Promise.all([get.body.json(), post.body.json()]);
  return c.json({
    ok: true,
    get_ok: get.statusCode === 200,
    post_ok: postData?.json?.tag === "undici-concurrent",
  });
});

Deno.serve(app.fetch);
