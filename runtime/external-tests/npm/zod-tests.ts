/**
 * Zod schema validation — compatibility tests
 */

import { z, ZodError } from "zod";
import type { TestResult } from "../../runners/lib/utils.js";

async function run(name: string, fn: () => void | Promise<void>): Promise<TestResult> {
  const t0 = performance.now();
  try {
    await fn();
    return { name, passed: true, skipped: false, duration: Math.round(performance.now() - t0) };
  } catch (e) {
    return {
      name, passed: false, skipped: false,
      error: e instanceof Error ? e.message : String(e),
      duration: Math.round(performance.now() - t0),
    };
  }
}

export async function runZodTests(): Promise<TestResult[]> {
  const results: TestResult[] = [];

  // ── Primitive schemas ────────────────────────────────────────────────────

  results.push(await run("z.string parses a valid string", () => {
    const val = z.string().parse("hello");
    if (val !== "hello") throw new Error("wrong value");
  }));

  results.push(await run("z.string throws for non-string input", () => {
    try { z.string().parse(42); throw new Error("no throw"); }
    catch (e) { if (!(e instanceof ZodError)) throw new Error("not ZodError"); }
  }));

  results.push(await run("z.number parses integer and float", () => {
    if (z.number().parse(42)  !== 42)  throw new Error("int failed");
    if (z.number().parse(3.14) !== 3.14) throw new Error("float failed");
  }));

  results.push(await run("z.boolean parses true and false", () => {
    if (z.boolean().parse(true)  !== true)  throw new Error("true");
    if (z.boolean().parse(false) !== false) throw new Error("false");
  }));

  // ── Object schema ────────────────────────────────────────────────────────

  results.push(await run("z.object validates a nested object", () => {
    const UserSchema = z.object({
      id:    z.number(),
      email: z.string().email(),
      age:   z.number().min(0).max(120),
    });
    const user = UserSchema.parse({ id: 1, email: "a@b.com", age: 30 });
    if (user.id !== 1 || user.email !== "a@b.com" || user.age !== 30) {
      throw new Error("wrong output");
    }
  }));

  results.push(await run("z.object strips unknown keys by default", () => {
    const schema = z.object({ name: z.string() });
    const out    = schema.parse({ name: "flux", extra: "ignored" });
    if ("extra" in out) throw new Error("extra key not stripped");
  }));

  results.push(await run("z.object reports all errors at once", () => {
    const schema = z.object({ a: z.string(), b: z.number() });
    try {
      schema.parse({ a: 1, b: "x" });
      throw new Error("no throw");
    } catch (e) {
      if (!(e instanceof ZodError)) throw new Error("not ZodError");
      if (e.issues.length < 2) throw new Error("expected 2+ issues");
    }
  }));

  // ── Array + union ────────────────────────────────────────────────────────

  results.push(await run("z.array validates array of strings", () => {
    const out = z.array(z.string()).parse(["a", "b", "c"]);
    if (out.length !== 3 || out[0] !== "a") throw new Error("wrong output");
  }));

  results.push(await run("z.union accepts either type", () => {
    const schema = z.union([z.string(), z.number()]);
    if (schema.parse("hi")  !== "hi") throw new Error("string branch");
    if (schema.parse(99)    !== 99)   throw new Error("number branch");
  }));

  // ── Optional + nullable ──────────────────────────────────────────────────

  results.push(await run("z.optional accepts undefined", () => {
    const schema = z.object({ x: z.string().optional() });
    const out    = schema.parse({});
    if ("x" in out && out.x !== undefined) throw new Error("x should be undefined");
  }));

  results.push(await run("z.nullable accepts null", () => {
    const schema = z.nullable(z.string());
    if (schema.parse(null)   !== null)    throw new Error("null not accepted");
    if (schema.parse("foo")  !== "foo")   throw new Error("string not accepted");
  }));

  // ── Transform + refinement ───────────────────────────────────────────────

  results.push(await run("z.transform converts parsed value", () => {
    const schema = z.string().transform(s => s.toUpperCase());
    if (schema.parse("hello") !== "HELLO") throw new Error("transform failed");
  }));

  results.push(await run("z.refine rejects with custom message", () => {
    const schema = z.number().refine(n => n > 0, "must be positive");
    try {
      schema.parse(-1);
      throw new Error("no throw");
    } catch (e) {
      if (!(e instanceof ZodError)) throw new Error("not ZodError");
      if (!e.issues[0].message.includes("positive")) throw new Error("wrong message");
    }
  }));

  // ── safeParse ────────────────────────────────────────────────────────────

  results.push(await run("safeParse returns success=true on valid input", () => {
    const res = z.string().safeParse("ok");
    if (!res.success) throw new Error("expected success");
    if (res.data !== "ok") throw new Error("wrong data");
  }));

  results.push(await run("safeParse returns success=false on invalid input", () => {
    const res = z.string().safeParse(99);
    if (res.success) throw new Error("expected failure");
    if (!res.error) throw new Error("missing error");
  }));

  // ── Enum ─────────────────────────────────────────────────────────────────

  results.push(await run("z.enum validates known values", () => {
    const schema = z.enum(["PENDING", "RUNNING", "DONE"]);
    if (schema.parse("PENDING") !== "PENDING") throw new Error("PENDING failed");
    try { schema.parse("OTHER"); throw new Error("no throw"); }
    catch (e) { if (!(e instanceof ZodError)) throw new Error("not ZodError"); }
  }));

  return results;
}
