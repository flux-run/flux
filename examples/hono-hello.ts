// @ts-nocheck
import { Hono } from "npm:hono";

const app = new Hono();

app.get("/", (c) => c.text("hello from hono on flux"));
app.get("/app-health", (c) => c.json({ ok: true }));

Deno.serve(app.fetch);