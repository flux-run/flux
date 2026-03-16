/**
 * jsonwebtoken — compatibility tests
 *
 * Tests sign / verify / decode without external key files.
 * All key material is generated inline so the tests are fully self-contained.
 */

import jwt from "jsonwebtoken";
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

const SECRET  = "flux-super-secret-key-for-testing-only";
const PAYLOAD = { userId: 42, role: "admin" };

export async function runJwtTests(): Promise<TestResult[]> {
  const results: TestResult[] = [];

  // ── Sign ─────────────────────────────────────────────────────────────────

  results.push(await run("sign returns a three-part JWT string", () => {
    const token = jwt.sign(PAYLOAD, SECRET);
    const parts  = token.split(".");
    if (parts.length !== 3) throw new Error(`expected 3 parts, got ${parts.length}`);
  }));

  results.push(await run("sign with HS256 algorithm produces valid header", () => {
    const token  = jwt.sign(PAYLOAD, SECRET, { algorithm: "HS256" });
    const header = JSON.parse(Buffer.from(token.split(".")[0], "base64url").toString());
    if (header.alg !== "HS256") throw new Error(`alg: ${header.alg}`);
    if (header.typ !== "JWT")   throw new Error(`typ: ${header.typ}`);
  }));

  results.push(await run("sign embeds payload claims in token body", () => {
    const token   = jwt.sign(PAYLOAD, SECRET);
    const decoded = jwt.decode(token) as { userId: number; role: string };
    if (decoded.userId !== 42)      throw new Error("userId missing");
    if (decoded.role   !== "admin") throw new Error("role missing");
  }));

  results.push(await run("sign with expiresIn adds exp claim", () => {
    const token = jwt.sign(PAYLOAD, SECRET, { expiresIn: "1h" });
    const body  = jwt.decode(token) as { exp: number; iat: number };
    if (typeof body.exp !== "number") throw new Error("exp not set");
    if (body.exp - body.iat < 3590)   throw new Error("exp too small");
  }));

  // ── Verify ───────────────────────────────────────────────────────────────

  results.push(await run("verify returns payload for a valid token", () => {
    const token   = jwt.sign(PAYLOAD, SECRET);
    const decoded = jwt.verify(token, SECRET) as { userId: number };
    if (decoded.userId !== 42) throw new Error("userId wrong");
  }));

  results.push(await run("verify throws JsonWebTokenError for wrong secret", () => {
    const token = jwt.sign(PAYLOAD, SECRET);
    try {
      jwt.verify(token, "wrong-secret");
      throw new Error("should have thrown");
    } catch (e) {
      if (!(e instanceof jwt.JsonWebTokenError)) throw e;
    }
  }));

  results.push(await run("verify throws TokenExpiredError for expired token", async () => {
    // Sign a token that expired 10 seconds ago
    const token = jwt.sign(PAYLOAD, SECRET, { expiresIn: -10 });
    try {
      jwt.verify(token, SECRET);
      throw new Error("should have thrown");
    } catch (e) {
      if (!(e instanceof jwt.TokenExpiredError)) throw new Error(`expected TokenExpiredError, got ${(e as Error).name}`);
    }
  }));

  // ── Decode ───────────────────────────────────────────────────────────────

  results.push(await run("decode returns payload without verification", () => {
    const token   = jwt.sign(PAYLOAD, SECRET);
    const decoded = jwt.decode(token) as { userId: number };
    if (decoded.userId !== 42) throw new Error("userId wrong");
  }));

  results.push(await run("decode with { complete: true } returns header+payload+signature", () => {
    const token    = jwt.sign(PAYLOAD, SECRET);
    const complete = jwt.decode(token, { complete: true });
    if (!complete || typeof complete !== "object") throw new Error("not an object");
    const c = complete as { header: unknown; payload: unknown; signature: unknown };
    if (!c.header)    throw new Error("no header");
    if (!c.payload)   throw new Error("no payload");
    if (!c.signature) throw new Error("no signature");
  }));

  results.push(await run("sign + verify round-trip with custom claims", () => {
    const token = jwt.sign(
      { sub: "user-123", requestId: "req-abc", org: "acme" },
      SECRET,
      { issuer: "flux", audience: "api" },
    );
    const payload = jwt.verify(token, SECRET, { issuer: "flux", audience: "api" }) as {
      sub: string; requestId: string; org: string;
    };
    if (payload.sub       !== "user-123") throw new Error("sub wrong");
    if (payload.requestId !== "req-abc")  throw new Error("requestId wrong");
    if (payload.org       !== "acme")     throw new Error("org wrong");
  }));

  return results;
}
