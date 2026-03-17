import { Hono } from "npm:hono";

const app = new Hono();

app.get("/", (c) => c.text("hello from hono on flux"));
app.get("/health", (c) => c.json({ ok: true }));

Deno.serve(app.fetch);