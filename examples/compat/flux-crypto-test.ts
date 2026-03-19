import { Hono } from "npm:hono";

const app = new Hono();

app.get("/", async (c) => {
  const hash = await crypto.subtle.digest("SHA-256", new TextEncoder().encode("test"));
  const hex = Array.from(new Uint8Array(hash)).map(b => b.toString(16).padStart(2, "0")).join("");
  return c.json({ ok: true, hex });
});

Deno.serve(app.fetch);
