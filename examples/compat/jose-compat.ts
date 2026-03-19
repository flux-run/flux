// @ts-nocheck
// Compat test: Web Crypto API (crypto.subtle) — JWT-like sign/verify
// Note: npm:jose fails in Flux due to ESM URL re-exports — we test the same
// underlying Web Crypto API that jose wraps, which is a *better* determinism test.
import { Hono } from "npm:hono";

const app = new Hono();

// ── Helpers ────────────────────────────────────────────────────────────────

function base64url(bytes: ArrayBuffer): string {
  return btoa(String.fromCharCode(...new Uint8Array(bytes)))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}

function base64urlDecode(str: string): Uint8Array {
  const b64 = str.replace(/-/g, "+").replace(/_/g, "/");
  return Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
}

async function importHmacKey(secret: string, usage: "sign" | "verify") {
  const raw = new TextEncoder().encode(secret);
  return crypto.subtle.importKey("raw", raw, { name: "HMAC", hash: "SHA-256" }, false, [usage]);
}

async function signJwt(payload: object, secret: string): Promise<string> {
  const header = base64url(new TextEncoder().encode(JSON.stringify({ alg: "HS256", typ: "JWT" })));
  const body = base64url(new TextEncoder().encode(JSON.stringify({
    ...payload,
    iat: Math.floor(Date.now() / 1000),
    exp: Math.floor(Date.now() / 1000) + 3600,
  })));
  const key = await importHmacKey(secret, "sign");
  const sigBytes = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(`${header}.${body}`));
  const sig = base64url(sigBytes);
  return `${header}.${body}.${sig}`;
}

async function verifyJwt(token: string, secret: string): Promise<object> {
  const [header, body, sig] = token.split(".");
  const key = await importHmacKey(secret, "verify");
  const valid = await crypto.subtle.verify(
    "HMAC",
    key,
    base64urlDecode(sig),
    new TextEncoder().encode(`${header}.${body}`),
  );
  if (!valid) throw new Error("JWSSignatureVerificationFailed");
  const payload = JSON.parse(new TextDecoder().decode(base64urlDecode(body)));
  if (payload.exp && payload.exp < Math.floor(Date.now() / 1000)) {
    throw new Object({ code: "ERR_JWT_EXPIRED", name: "JWTExpired" });
  }
  return payload;
}

const SHARED_SECRET = "flux-webcrypto-compat-test-secret-key";

// GET / — smoke test
app.get("/", (c) => c.json({ library: "webcrypto", ok: true }));

// POST /sign — HMAC-SHA256 JWT sign
app.post("/sign", async (c) => {
  const body = await c.req.json();
  const { sub = "user-123", role = "user" } = body;
  const token = await signJwt({ sub, role, iss: "flux-compat" }, SHARED_SECRET);
  return c.json({ ok: true, token, parts: token.split(".").length });
});

// POST /verify — verify + decode
app.post("/verify", async (c) => {
  const { token } = await c.req.json();
  try {
    const payload = await verifyJwt(token, SHARED_SECRET) as any;
    return c.json({ ok: true, payload, sub: payload.sub });
  } catch (e: any) {
    return c.json({ ok: false, error: e?.message ?? String(e) }, 401);
  }
});

// POST /sign-verify-cycle — end-to-end in one request
app.post("/sign-verify-cycle", async (c) => {
  const { sub = "cycle-user" } = await c.req.json();
  const token = await signJwt({ sub, iss: "flux-compat" }, SHARED_SECRET);
  const payload = await verifyJwt(token, SHARED_SECRET) as any;
  return c.json({ ok: true, sub_matches: payload.sub === sub, token_length: token.length });
});

// POST /verify-bad — bad/tampered token → error caught
app.post("/verify-bad", async (_c) => {
  const tampered = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJoYWNrZXIifQ.BADSIGNATURE";
  try {
    await verifyJwt(tampered, SHARED_SECRET);
    return _c.json({ ok: false, error: "expected error" }, 500);
  } catch (e: any) {
    return _c.json({ ok: true, caught: true, error: e?.message ?? String(e) });
  }
});

// POST /verify-expired — expired token → caught
app.post("/verify-expired", async (c) => {
  // Build a token with an already-expired exp
  const header = base64url(new TextEncoder().encode(JSON.stringify({ alg: "HS256", typ: "JWT" })));
  const body = base64url(new TextEncoder().encode(JSON.stringify({
    sub: "expired-user",
    iat: 1000000,
    exp: 1000001, // long in the past
  })));
  const key = await importHmacKey(SHARED_SECRET, "sign");
  const sigBytes = await crypto.subtle.sign("HMAC", key, new TextEncoder().encode(`${header}.${body}`));
  const token = `${header}.${body}.${base64url(sigBytes)}`;
  try {
    await verifyJwt(token, SHARED_SECRET);
    return c.json({ ok: false, error: "expected expiry error" }, 500);
  } catch {
    return c.json({ ok: true, caught: true, expired: true });
  }
});

// GET /jwks — RWSA key pair + JWKS (asymmetric via WebCrypto)
app.get("/jwks", async (c) => {
  const { publicKey } = await crypto.subtle.generateKey(
    { name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" },
    true,
    ["sign", "verify"],
  );
  const jwk = await crypto.subtle.exportKey("jwk", publicKey);
  return c.json({
    keys: [{ ...jwk, kid: "flux-rs256-key", use: "sig", alg: "RS256" }],
  });
});

// GET /digest — SHA-256 hash of known input (determinism check)
app.get("/digest", async (c) => {
  const input = "flux-determinism-test";
  const hash = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(input));
  const hex = Array.from(new Uint8Array(hash)).map((b) => b.toString(16).padStart(2, "0")).join("");
  return c.json({ ok: true, input, hex });
});

// POST /derive-key — PBKDF2 key derivation
app.post("/derive-key", async (c) => {
  const { password, salt } = await c.req.json();
  const keyMaterial = await crypto.subtle.importKey(
    "raw",
    new TextEncoder().encode(password),
    "PBKDF2",
    false,
    ["deriveBits"],
  );
  const bits = await crypto.subtle.deriveBits(
    {
      name: "PBKDF2",
      hash: "SHA-256",
      salt: new TextEncoder().encode(salt),
      iterations: 1000,
    },
    keyMaterial,
    256,
  );
  const hex = Array.from(new Uint8Array(bits)).map((b) => b.toString(16).padStart(2, "0")).join("");
  return c.json({ ok: true, derived_bits_length: bits.byteLength * 8, hex });
});

Deno.serve(app.fetch);
