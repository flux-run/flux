// @ts-nocheck
// Compat test: Zod validation (pure JS — no IO required)
import { Hono } from "npm:hono";
import { z } from "npm:zod";

const app = new Hono();

// Schemas
const UserSchema = z.object({
  name: z.string().min(1),
  email: z.string().email(),
  age: z.number().int().min(0).max(150).optional(),
});

const PaginationSchema = z.object({
  page: z.coerce.number().int().min(1).default(1),
  limit: z.coerce.number().int().min(1).max(100).default(20),
});

// GET / — smoke test
app.get("/", (c) => c.json({ library: "zod", ok: true }));

// POST /validate-user — parse + validate a user body
app.post("/validate-user", async (c) => {
  const body = await c.req.json();
  const result = UserSchema.safeParse(body);
  if (!result.success) {
    return c.json({ ok: false, errors: result.error.flatten() }, 422);
  }
  return c.json({ ok: true, user: result.data });
});

// POST /validate-bad — intentionally bad payload
app.post("/validate-bad", async (c) => {
  const body = await c.req.json();
  const result = UserSchema.safeParse(body);
  return c.json({ ok: result.success, errors: result.success ? null : result.error.flatten() });
});

// GET /paginate — query param coercion with defaults
app.get("/paginate", (c) => {
  const query = { page: c.req.query("page"), limit: c.req.query("limit") };
  const result = PaginationSchema.safeParse(query);
  if (!result.success) {
    return c.json({ ok: false, errors: result.error.flatten() }, 422);
  }
  return c.json({ ok: true, pagination: result.data });
});

// POST /transform — zod transform pipeline
app.post("/transform", async (c) => {
  const body = await c.req.json();
  const schema = z.object({ value: z.string().trim().toLowerCase().min(1) });
  const result = schema.safeParse(body);
  if (!result.success) return c.json({ ok: false }, 422);
  return c.json({ ok: true, transformed: result.data });
});

Deno.serve(app.fetch);
