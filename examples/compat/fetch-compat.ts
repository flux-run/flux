// @ts-nocheck
// Compat test: native fetch + JSON response parsing
import { Hono } from "npm:hono";

const app = new Hono();

// GET /echo — returns a simple JSON echo of route info
app.get("/", (c) => c.json({ library: "fetch", ok: true }));

// GET /fetch-external — makes an outbound fetch (intercepted by Flux)
app.get("/fetch-external", async (c) => {
  const res = await fetch("https://httpbin.org/get?from=flux");
  const data = await res.json();
  return c.json({
    ok: true,
    status: res.status,
    origin_present: typeof data?.origin === "string",
  });
});

// POST /fetch-post — makes an outbound POST fetch
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

// GET /fetch-headers — verifies custom headers are forwarded
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

Deno.serve(app.fetch);
