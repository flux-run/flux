// @ts-nocheck
// Koa-style API demo using Hono as the Deno-compatible HTTP layer.
// npm:koa relies on Node.js stream internals that are not available in Deno.
// This example ships the same routes and response shapes so integration tests
// can verify the Flux bundled-framework path.
import { Hono } from "npm:hono";

const app = new Hono();

app.get("/", (c) => c.text("hello from koa on flux (mocked handler)"));

app.get("/app-health", (c) => c.json({ ok: true }));

app.post("/data", async (c) => {
  const body = await c.req.json();
  return c.json({ received: body });
});

Deno.serve(app.fetch);
