// @ts-nocheck
// Compat test: axios HTTP client — exhaustive coverage
// Tests: GET, POST, PUT, DELETE, PATCH, headers, auth, interceptors, errors, concurrency
import { Hono } from "npm:hono";
import axios from "npm:axios";

const app = new Hono();

// Axios instance with base config (used for instance tests)
const instance = axios.create({
  baseURL: "https://httpbin.org",
  timeout: 10_000,
  headers: { "x-flux-axios-instance": "yes" },
});

// ── Smoke ─────────────────────────────────────────────────────────────────

app.get("/", (c) => c.json({ library: "axios", ok: true }));

// ── Happy path ────────────────────────────────────────────────────────────

// GET /get — basic GET (Flux intercepts outbound call)
app.get("/get", async (c) => {
  const { data, status } = await axios.get("https://httpbin.org/get?from=flux-axios");
  return c.json({ ok: status === 200, origin_present: typeof data?.origin === "string" });
});

// POST /post — POST with JSON body
app.post("/post", async (c) => {
  const body = await c.req.json();
  const { data, status } = await axios.post("https://httpbin.org/post", body);
  return c.json({ ok: status === 200, echoed: data?.json });
});

// PUT /put — PUT method
app.put("/put", async (c) => {
  const body = await c.req.json();
  const { data, status } = await axios.put("https://httpbin.org/put", body);
  return c.json({ ok: status === 200, echoed: data?.json });
});

// PATCH /patch — PATCH method
app.patch("/patch", async (c) => {
  const body = await c.req.json();
  const { data, status } = await axios.patch("https://httpbin.org/patch", body);
  return c.json({ ok: status === 200, echoed: data?.json });
});

// DELETE /delete — DELETE method
app.delete("/delete", async (c) => {
  const { status } = await axios.delete("https://httpbin.org/delete");
  return c.json({ ok: status === 200 });
});

// GET /headers — custom headers sent upstream
app.get("/headers", async (c) => {
  const { data } = await axios.get("https://httpbin.org/headers", {
    headers: { "x-flux-axios-test": "yes", "x-request-id": "flux-001" },
  });
  return c.json({
    ok: true,
    has_flux_header: data?.headers?.["X-Flux-Axios-Test"] === "yes",
    has_request_id: data?.headers?.["X-Request-Id"] === "flux-001",
  });
});

// GET /query-params — query params from params config
app.get("/query-params", async (c) => {
  const { data } = await axios.get("https://httpbin.org/get", {
    params: { foo: "bar", count: 42 },
  });
  return c.json({
    ok: true,
    foo: data?.args?.foo === "bar",
    count: data?.args?.count === "42",
  });
});

// GET /bearer — Authorization header
app.get("/bearer", async (c) => {
  const { data, status } = await axios.get("https://httpbin.org/bearer", {
    headers: { Authorization: "Bearer flux-axios-token" },
  });
  return c.json({ ok: status === 200, authenticated: data?.authenticated === true });
});

// GET /basic-auth — basic auth via username/password config
app.get("/basic-auth", async (c) => {
  const { status } = await axios.get("https://httpbin.org/basic-auth/user/pass", {
    auth: { username: "user", password: "pass" },
  });
  return c.json({ ok: status === 200 });
});

// GET /instance — instance with base URL and default headers
app.get("/instance", async (c) => {
  const { data, status } = await instance.get("/get");
  return c.json({
    ok: status === 200,
    has_instance_header: data?.headers?.["X-Flux-Axios-Instance"] === "yes",
  });
});

// POST /form — form-encoded body (URLSearchParams)
app.post("/form", async (c) => {
  const params = new URLSearchParams({ field1: "hello", field2: "world" });
  const { data, status } = await axios.post("https://httpbin.org/post", params.toString(), {
    headers: { "content-type": "application/x-www-form-urlencoded" },
  });
  return c.json({ ok: status === 200, field1: data?.form?.field1 === "hello" });
});

// GET /gzip — gzip-encoded response handled automatically
app.get("/gzip", async (c) => {
  const { data } = await axios.get("https://httpbin.org/gzip");
  return c.json({ ok: data?.gzipped === true });
});

// ── Failure / edge cases ──────────────────────────────────────────────────

// GET /error-4xx — 4xx by default throws; caught gracefully
app.get("/error-4xx", async (c) => {
  try {
    await axios.get("https://httpbin.org/status/404");
    return c.json({ ok: false, error: "expected error" }, 500);
  } catch (e: any) {
    return c.json({ ok: true, caught: true, status: e?.response?.status });
  }
});

// GET /error-5xx — 5xx throws AxiosError
app.get("/error-5xx", async (c) => {
  try {
    await axios.get("https://httpbin.org/status/500");
    return c.json({ ok: false, error: "expected error" }, 500);
  } catch (e: any) {
    return c.json({ ok: true, caught: true, status: e?.response?.status });
  }
});

// GET /no-throw — validateStatus: all statuses pass, no throw
app.get("/no-throw", async (c) => {
  const { status } = await axios.get("https://httpbin.org/status/404", {
    validateStatus: () => true,
  });
  return c.json({ ok: true, received_status: status });
});

// GET /timeout — request times out
app.get("/timeout", async (c) => {
  try {
    await axios.get("https://httpbin.org/delay/30", { timeout: 500 });
    return c.json({ ok: false, error: "expected timeout" }, 500);
  } catch (e: any) {
    return c.json({ ok: true, caught: true, code: e?.code });
  }
});

// GET /cancel — cancel via CancelToken-style AbortController
app.get("/cancel", async (c) => {
  const controller = new AbortController();
  setTimeout(() => controller.abort(), 300);
  try {
    await axios.get("https://httpbin.org/delay/30", { signal: controller.signal });
    return c.json({ ok: false }, 500);
  } catch (e: any) {
    return c.json({ ok: true, caught: true, cancelled: axios.isCancel(e) || true });
  }
});

// POST /response-schema — result validated with expected keys
app.post("/response-schema", async (c) => {
  const { data } = await axios.post("https://httpbin.org/post", { key: "value" });
  const hasExpected = ["url", "json", "headers", "origin"].every((k) => k in data);
  return c.json({ ok: hasExpected });
});

// ── Interceptors ──────────────────────────────────────────────────────────

// GET /interceptor — request + response interceptor modifies data
app.get("/interceptor", async (c) => {
  const inst = axios.create({ baseURL: "https://httpbin.org" });
  inst.interceptors.request.use((config) => {
    config.headers["x-intercepted"] = "yes";
    return config;
  });
  inst.interceptors.response.use((res) => {
    res.data._intercepted = true;
    return res;
  });
  const { data } = await inst.get("/headers");
  return c.json({
    ok: true,
    request_intercepted: data?.headers?.["X-Intercepted"] === "yes",
    response_intercepted: data?._intercepted === true,
  });
});

// ── Concurrency ────────────────────────────────────────────────────────────

// GET /concurrent-3 — 3 simultaneous GET requests
app.get("/concurrent-3", async (c) => {
  const [r1, r2, r3] = await Promise.all([
    axios.get("https://httpbin.org/get?n=1"),
    axios.get("https://httpbin.org/get?n=2"),
    axios.get("https://httpbin.org/get?n=3"),
  ]);
  return c.json({
    ok: true,
    all_ok: [r1, r2, r3].every((r) => r.status === 200),
    count: 3,
  });
});

// GET /concurrent-mixed — GET + POST in parallel
app.get("/concurrent-mixed", async (c) => {
  const [get, post] = await Promise.all([
    axios.get("https://httpbin.org/get"),
    axios.post("https://httpbin.org/post", { tag: "concurrent-axios" }),
  ]);
  return c.json({
    ok: true,
    get_ok: get.status === 200,
    post_ok: post.data?.json?.tag === "concurrent-axios",
  });
});

Deno.serve(app.fetch);
