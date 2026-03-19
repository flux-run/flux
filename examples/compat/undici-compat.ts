// @ts-nocheck
// Compat test: undici HTTP client (Node 18+ stdlib standard)
import { Hono } from "npm:hono";
// undici is bundled into Node.js — available as a direct npm package
import { request, fetch as undiciFetch } from "npm:undici";

const app = new Hono();

// GET / — smoke test
app.get("/", (c) => c.json({ library: "undici", ok: true }));

// GET /undici-request — low-level undici `request()` API
app.get("/undici-request", async (c) => {
  const { statusCode, body } = await request("https://httpbin.org/get?from=flux-undici");
  const data = await body.json();
  return c.json({
    ok: true,
    status: statusCode,
    origin_present: typeof data?.origin === "string",
  });
});

// POST /undici-post — undici POST with JSON body
app.post("/undici-post", async (c) => {
  const payload = await c.req.json();
  const { statusCode, body } = await request("https://httpbin.org/post", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(payload),
  });
  const data = await body.json();
  return c.json({ ok: statusCode === 200, echoed: data?.json });
});

// GET /undici-fetch — undici's WHATWG-compatible fetch (verifies interception)
app.get("/undici-fetch", async (c) => {
  const res = await undiciFetch("https://httpbin.org/get?from=flux-undici-fetch");
  const data = await res.json();
  return c.json({
    ok: true,
    status: res.status,
    origin_present: typeof (data as any)?.origin === "string",
  });
});

// GET /undici-headers — verify custom headers forwarded correctly
app.get("/undici-headers", async (c) => {
  const { statusCode, body } = await request("https://httpbin.org/headers", {
    headers: { "x-flux-undici-test": "hello" },
  });
  const data = await body.json();
  return c.json({
    ok: statusCode === 200,
    has_custom_header: typeof data?.headers?.["X-Flux-Undici-Test"] === "string",
  });
});

Deno.serve(app.fetch);
