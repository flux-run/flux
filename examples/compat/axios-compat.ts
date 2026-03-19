// @ts-nocheck
// Compat test: axios HTTP client
import { Hono } from "npm:hono";
import axios from "npm:axios";

const app = new Hono();

// GET / — smoke test
app.get("/", (c) => c.json({ library: "axios", ok: true }));

// GET /axios-get — axios GET request (Flux intercepts the outbound call)
app.get("/axios-get", async (c) => {
  const { data, status } = await axios.get("https://httpbin.org/get?from=flux-axios");
  return c.json({
    ok: true,
    status,
    origin_present: typeof data?.origin === "string",
  });
});

// POST /axios-post — axios POST with JSON body
app.post("/axios-post", async (c) => {
  const body = await c.req.json();
  const { data, status } = await axios.post("https://httpbin.org/post", body, {
    headers: { "content-type": "application/json" },
  });
  return c.json({ ok: status === 200, echoed: data?.json });
});

// GET /axios-headers — axios with custom headers
app.get("/axios-headers", async (c) => {
  const { data } = await axios.get("https://httpbin.org/headers", {
    headers: { "x-flux-axios-test": "yes" },
  });
  return c.json({
    ok: true,
    has_custom_header: typeof data?.headers?.["X-Flux-Axios-Test"] === "string",
  });
});

// GET /axios-error — axios non-2xx does not throw (validateStatus)
app.get("/axios-error", async (c) => {
  const { status } = await axios.get("https://httpbin.org/status/404", {
    validateStatus: () => true,
  });
  return c.json({ ok: true, received_status: status });
});

Deno.serve(app.fetch);
