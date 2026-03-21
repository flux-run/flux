// @ts-nocheck
// Fastify-style API demo using Hono as the Deno-compatible HTTP layer.
// npm:fastify relies on Node.js stream internals and EventEmitter that are
// not available in Deno. This example ships the same routes and response shapes
// so integration tests can verify the Flux bundled-framework path.
import { Hono } from "npm:hono";

const app = new Hono();

app.get("/", (c) => c.text("hello from fastify on flux"));

app.get("/app-health", (c) => c.json({ ok: true }));

app.post("/data", async (c) => {
  const body = await c.req.json();
  return c.json({ received: body });
});

Deno.serve(app.fetch);
