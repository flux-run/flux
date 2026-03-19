import { Hono } from "npm:hono";
const app = new Hono();
app.get("/", async (c) => c.json({ ok: true, version: 1 }));
Deno.serve(app.fetch);
