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

// ── Failure cases ──────────────────────────────────────────────────────────

// POST /validate-strict — strict mode rejects unknown keys
app.post("/validate-strict", async (c) => {
  const body = await c.req.json();
  const StrictUser = UserSchema.strict();
  const result = StrictUser.safeParse(body);
  return c.json({
    ok: result.success,
    errors: result.success ? null : result.error.flatten(),
  });
});

// POST /validate-nested — nested object schema
app.post("/validate-nested", async (c) => {
  const body = await c.req.json();
  const AddressSchema = z.object({
    street: z.string().min(1),
    city: z.string().min(1),
    zip: z.string().regex(/^\d{5}$/),
  });
  const schema = z.object({
    user: UserSchema,
    address: AddressSchema,
  });
  const result = schema.safeParse(body);
  if (!result.success) {
    return c.json({ ok: false, errors: result.error.flatten() }, 422);
  }
  return c.json({ ok: true, data: result.data });
});

// POST /validate-union — union type schema
app.post("/validate-union", async (c) => {
  const body = await c.req.json();
  const schema = z.union([
    z.object({ type: z.literal("email"), value: z.string().email() }),
    z.object({ type: z.literal("phone"), value: z.string().regex(/^\+?[\d\s-]{7,}$/) }),
  ]);
  const result = schema.safeParse(body);
  return c.json({ ok: result.success, data: result.success ? result.data : null });
});

// POST /validate-custom — custom refinement
app.post("/validate-custom", async (c) => {
  const body = await c.req.json();
  const schema = z
    .object({ password: z.string(), confirm: z.string() })
    .refine((d) => d.password === d.confirm, {
      message: "Passwords do not match",
      path: ["confirm"],
    });
  const result = schema.safeParse(body);
  return c.json({
    ok: result.success,
    errors: result.success ? null : result.error.flatten(),
  });
});

Deno.serve(app.fetch);
