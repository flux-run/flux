// @ts-nocheck
// Compat test: Zod validation — exhaustive coverage
// Tests: primitive types, objects, arrays, unions, intersections, discriminated unions,
//        transforms, refinements, optional/default/nullable, strict mode, error formatting
import { Hono } from "npm:hono";
import { z } from "npm:zod";

const app = new Hono();

// ── Smoke ─────────────────────────────────────────────────────────────────

app.get("/", (c) => c.json({ library: "zod", ok: true }));

// ── Reusable schemas ──────────────────────────────────────────────────────

const UserSchema = z.object({
  name: z.string().min(1).max(100),
  email: z.string().email(),
  age: z.number().int().min(0).max(150).optional(),
  role: z.enum(["admin", "user", "guest"]).default("user"),
});

const AddressSchema = z.object({
  street: z.string().min(1),
  city: z.string().min(1),
  zip: z.string().regex(/^\d{5}$/, "ZIP must be 5 digits"),
  country: z.string().length(2).toUpperCase(),
});

const PaginationSchema = z.object({
  page: z.coerce.number().int().min(1).default(1),
  limit: z.coerce.number().int().min(1).max(100).default(20),
});

// ── Primitive types ───────────────────────────────────────────────────────

// POST /primitives — validates string, number, boolean, date, null, undefined
app.post("/primitives", async (c) => {
  const body = await c.req.json();
  const schema = z.object({
    str: z.string(),
    num: z.number(),
    bool: z.boolean(),
    nullable: z.string().nullable(),
    optional: z.string().optional(),
  });
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null, errors: r.success ? null : r.error.flatten() });
});

// POST /coerce — coercion from strings to number/boolean/date
app.post("/coerce", async (c) => {
  const body = await c.req.json();
  const schema = z.object({
    num: z.coerce.number(),
    bool: z.coerce.boolean(),
    date: z.coerce.date(),
  });
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, data: r.success ? { num: r.data.num, bool: r.data.bool, date_valid: r.data.date instanceof Date } : null });
});

// ── String validations ────────────────────────────────────────────────────

// POST /strings — exhaustive string validators
app.post("/strings", async (c) => {
  const body = await c.req.json();
  const schema = z.object({
    email: z.string().email(),
    url: z.string().url(),
    uuid: z.string().uuid(),
    min3: z.string().min(3),
    max10: z.string().max(10),
    regex: z.string().regex(/^[a-z]+$/),
    startsWith: z.string().startsWith("flux-"),
    endsWith: z.string().endsWith("-ok"),
    trimmed: z.string().trim(),
    lower: z.string().toLowerCase(),
    upper: z.string().toUpperCase(),
  });
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null, errors: r.success ? null : r.error.flatten() });
});

// POST /string-bad — invalid strings produce clear error messages
app.post("/string-bad", async (c) => {
  const body = await c.req.json();
  const schema = z.object({ email: z.string().email(), url: z.string().url() });
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, errors: r.success ? null : r.error.flatten() });
});

// ── Object schemas ─────────────────────────────────────────────────────────

// POST /object — validate user object
app.post("/object", async (c) => {
  const body = await c.req.json();
  const r = UserSchema.safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null, errors: r.success ? null : r.error.flatten() });
});

// POST /object-strict — reject unknown keys
app.post("/object-strict", async (c) => {
  const body = await c.req.json();
  const r = UserSchema.strict().safeParse(body);
  return c.json({ ok: r.success, errors: r.success ? null : r.error.flatten() });
});

// POST /object-passthrough — allow unknown keys through
app.post("/object-passthrough", async (c) => {
  const body = await c.req.json();
  const r = UserSchema.passthrough().safeParse(body);
  return c.json({ ok: r.success, keys: r.success ? Object.keys(r.data) : [] });
});

// POST /nested — deeply nested object
app.post("/nested", async (c) => {
  const body = await c.req.json();
  const schema = z.object({ user: UserSchema, address: AddressSchema });
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null, errors: r.success ? null : r.error.flatten() });
});

// POST /partial — all fields optional via .partial()
app.post("/partial", async (c) => {
  const body = await c.req.json();
  const r = UserSchema.partial().safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null });
});

// POST /pick — only name + email
app.post("/pick", async (c) => {
  const body = await c.req.json();
  const r = UserSchema.pick({ name: true, email: true }).safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null });
});

// POST /omit — everything except email
app.post("/omit", async (c) => {
  const body = await c.req.json();
  const r = UserSchema.omit({ email: true }).safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null });
});

// ── Arrays ─────────────────────────────────────────────────────────────────

// POST /array — array of strings
app.post("/array", async (c) => {
  const body = await c.req.json();
  const schema = z.array(z.string().min(1)).min(1).max(20);
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null, errors: r.success ? null : r.error.flatten() });
});

// POST /array-objects — array of user objects
app.post("/array-objects", async (c) => {
  const body = await c.req.json();
  const schema = z.array(UserSchema).nonempty();
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, count: r.success ? r.data.length : 0, errors: r.success ? null : r.error.flatten() });
});

// POST /tuple — tuple with fixed-type positions
app.post("/tuple", async (c) => {
  const body = await c.req.json();
  const schema = z.tuple([z.string(), z.number(), z.boolean()]);
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null });
});

// ── Unions & discriminated unions ──────────────────────────────────────────

// POST /union — string OR number
app.post("/union", async (c) => {
  const body = await c.req.json();
  const schema = z.union([z.string(), z.number()]);
  const r = schema.safeParse(body?.value);
  return c.json({ ok: r.success, data: r.success ? r.data : null, type: r.success ? typeof r.data : null });
});

// POST /union-objects — email or phone identifier
app.post("/union-objects", async (c) => {
  const body = await c.req.json();
  const schema = z.union([
    z.object({ type: z.literal("email"), value: z.string().email() }),
    z.object({ type: z.literal("phone"), value: z.string().regex(/^\+?[\d\s\-()]{7,}$/) }),
  ]);
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null });
});

// POST /discriminated — discriminated union on "type" field
app.post("/discriminated", async (c) => {
  const body = await c.req.json();
  const schema = z.discriminatedUnion("type", [
    z.object({ type: z.literal("create"), name: z.string() }),
    z.object({ type: z.literal("update"), id: z.number(), name: z.string() }),
    z.object({ type: z.literal("delete"), id: z.number() }),
  ]);
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null, errors: r.success ? null : r.error.flatten() });
});

// ── Transforms ────────────────────────────────────────────────────────────

// POST /transform-complex — trim + lowercase + parse to int (original complex transform)
app.post("/transform-complex", async (c) => {
  const body = await c.req.json();
  const schema = z.object({
    tag: z.string().trim().toLowerCase(),
    count: z.string().transform((v) => parseInt(v, 10)),
    doubled: z.number().transform((v) => v * 2),
  });
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null });
});

// POST /preprocess — preprocess before validation
app.post("/preprocess", async (c) => {
  const body = await c.req.json();
  const schema = z.preprocess(
    (v: any) => ({ ...v, name: v?.name?.trim() }),
    UserSchema.pick({ name: true, email: true }),
  );
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null });
});

// ── Refinements ────────────────────────────────────────────────────────────

// POST /refine-password — passwords must match
app.post("/refine-password", async (c) => {
  const body = await c.req.json();
  const schema = z
    .object({ password: z.string().min(8), confirm: z.string() })
    .refine((d) => d.password === d.confirm, { message: "Passwords do not match", path: ["confirm"] });
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, errors: r.success ? null : r.error.flatten() });
});

// POST /refine-range — min must be < max
app.post("/refine-range", async (c) => {
  const body = await c.req.json();
  const schema = z
    .object({ min: z.number(), max: z.number() })
    .refine((d) => d.min < d.max, { message: "min must be less than max" });
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, errors: r.success ? null : r.error.flatten() });
});

// POST /superrefine — multiple custom errors
app.post("/superrefine", async (c) => {
  const body = await c.req.json();
  const schema = z.object({ username: z.string(), age: z.number() }).superRefine((val, ctx) => {
    if (val.username.length < 3) {
      ctx.addIssue({ code: z.ZodIssueCode.custom, message: "username too short", path: ["username"] });
    }
    if (val.age < 18) {
      ctx.addIssue({ code: z.ZodIssueCode.custom, message: "must be 18+", path: ["age"] });
    }
  });
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, errors: r.success ? null : r.error.flatten() });
});

// ── Optional / nullable / default ─────────────────────────────────────────

// POST /optional-nullable — optional vs nullable behavior
app.post("/optional-nullable", async (c) => {
  const body = await c.req.json();
  const schema = z.object({
    required: z.string(),
    optional: z.string().optional(),
    nullable: z.string().nullable(),
    default_val: z.string().default("flux"),
    nullish: z.string().nullish(),
  });
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null, errors: r.success ? null : r.error.flatten() });
});

// ── Query params (coerce) ─────────────────────────────────────────────────

// GET /paginate — typed query params with coercion and defaults
app.get("/paginate", (c) => {
  const q = { page: c.req.query("page"), limit: c.req.query("limit") };
  const r = PaginationSchema.safeParse(q);
  return c.json({ ok: r.success, pagination: r.success ? r.data : null, errors: r.success ? null : r.error.flatten() });
});

// ── Error formatting ──────────────────────────────────────────────────────

// POST /error-format — flatten vs format error shapes
app.post("/error-format", async (c) => {
  const body = await c.req.json();
  const r = UserSchema.safeParse(body);
  if (r.success) return c.json({ ok: true });
  return c.json({
    ok: false,
    flattened: r.error.flatten(),
    formatted: r.error.format(),
    issues: r.error.issues,
  });
});

// ── parse vs safeParse ────────────────────────────────────────────────────

// POST /parse-throws — z.parse() throws on invalid input
app.post("/parse-throws", async (c) => {
  const body = await c.req.json();
  try {
    const data = UserSchema.parse(body);
    return c.json({ ok: true, data });
  } catch (e: any) {
    return c.json({ ok: false, caught: true, is_zod_error: e?.name === "ZodError", issues_count: e?.errors?.length });
  }
});

// ── Aliases expected by the integration test runner ───────────────────────

// /validate-user — validates a user and returns { ok, user: { name } }
app.post("/validate-user", async (c) => {
  const body = await c.req.json();
  const r = UserSchema.pick({ name: true, email: true }).safeParse(body);
  return c.json({
    ok: r.success,
    user: r.success ? { name: r.data.name } : null,
    errors: r.success ? null : r.error.flatten(),
  });
});

// /validate-bad — invalid user input (empty name, bad email)
app.post("/validate-bad", async (c) => {
  const body = await c.req.json();
  const r = UserSchema.pick({ name: true, email: true }).safeParse(body);
  return c.json({ ok: r.success, errors: r.success ? null : r.error.flatten() });
});

// /validate-strict — strict mode: unknown keys rejected
app.post("/validate-strict", async (c) => {
  const body = await c.req.json();
  const r = UserSchema.pick({ name: true, email: true }).strict().safeParse(body);
  return c.json({ ok: r.success, errors: r.success ? null : r.error.flatten() });
});

// /validate-nested — nested user + address schema
app.post("/validate-nested", async (c) => {
  const body = await c.req.json();
  const schema = z.object({
    user: UserSchema.pick({ name: true, email: true }),
    address: z.object({
      street: z.string().min(1),
      city: z.string().min(1),
      zip: z.string().min(1),
    }),
  });
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, errors: r.success ? null : r.error.flatten() });
});

// /validate-union — union of email or phone identifier
app.post("/validate-union", async (c) => {
  const body = await c.req.json();
  const schema = z.union([
    z.object({ type: z.literal("email"), value: z.string().email() }),
    z.object({ type: z.literal("phone"), value: z.string().min(7) }),
  ]);
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, data: r.success ? r.data : null });
});

// /validate-custom — refine: password === confirm
app.post("/validate-custom", async (c) => {
  const body = await c.req.json();
  const schema = z
    .object({ password: z.string(), confirm: z.string() })
    .refine((d) => d.password === d.confirm, { message: "Passwords do not match" });
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, errors: r.success ? null : r.error.flatten() });
});

// /transform — trim + lowercase a string value
app.post("/transform", async (c) => {
  const body = await c.req.json();
  const schema = z.object({ value: z.string().trim().toLowerCase() });
  const r = schema.safeParse(body);
  return c.json({ ok: r.success, transformed: r.success ? r.data : null });
});

Deno.serve(app.fetch);
