// Compat test: Web Crypto API — JWT sign/verify + crypto primitives
// Uses crypto.subtle which is natively available in Flux + Web Standard APIs
import { Hono } from "npm:hono";

const app = new Hono();
const SECRET = "flux-webcrypto-compat-test-secret-key";

// ── Helpers ───────────────────────────────────────────────────────────────

function toBase64url(buf: ArrayBuffer): string {
  const bytes = new Uint8Array(buf);
  let s = "";
  for (let i = 0; i < bytes.length; i++) s += String.fromCharCode(bytes[i]);
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64url(str: string): Uint8Array {
  const b64 = str.replace(/-/g, "+").replace(/_/g, "/");
  const raw = atob(b64);
  const out = new Uint8Array(raw.length);
  for (let i = 0; i < raw.length; i++) out[i] = raw.charCodeAt(i);
  return out;
}

async function getHmacKey(secret: string, usage: KeyUsage): Promise<CryptoKey> {
  const raw = new TextEncoder().encode(secret);
  return crypto.subtle.importKey("raw", raw, { name: "HMAC", hash: "SHA-256" }, false, [usage]);
}

async function makeToken(payload: Record<string, unknown>, secret: string): Promise<string> {
  const header = toBase64url(new TextEncoder().encode(JSON.stringify({ alg: "HS256", typ: "JWT" })));
  const now = Math.floor(Date.now() / 1000);
  const body = toBase64url(new TextEncoder().encode(JSON.stringify({ ...payload, iat: now, exp: now + 3600 })));
  const signingInput = new TextEncoder().encode(header + "." + body);
  const key = await getHmacKey(secret, "sign");
  const sigBuf = await crypto.subtle.sign("HMAC", key, signingInput);
  return header + "." + body + "." + toBase64url(sigBuf);
}

async function checkToken(token: string, secret: string): Promise<Record<string, unknown>> {
  const parts = token.split(".");
  if (parts.length !== 3) throw new Error("Invalid token format");
  const [header, body, sig] = parts;
  const key = await getHmacKey(secret, "verify");
  const input = new TextEncoder().encode(header + "." + body);
  const sigBytes = fromBase64url(sig);
  const ok = await crypto.subtle.verify("HMAC", key, sigBytes, input);
  if (!ok) throw new Error("JWSSignatureVerificationFailed");
  const payload = JSON.parse(new TextDecoder().decode(fromBase64url(body))) as Record<string, unknown>;
  const now = Math.floor(Date.now() / 1000);
  if (typeof payload.exp === "number" && payload.exp < now) {
    throw new Error("JWTExpired");
  }
  return payload;
}

// ── Routes ────────────────────────────────────────────────────────────────

app.get("/", (c) => c.json({ library: "webcrypto", ok: true }));

app.post("/sign", async (c) => {
  const input = await c.req.json() as Record<string, unknown>;
  const sub = typeof input.sub === "string" ? input.sub : "user-123";
  const role = typeof input.role === "string" ? input.role : "user";
  const token = await makeToken({ sub, role, iss: "flux-compat" }, SECRET);
  return c.json({ ok: true, token, parts: token.split(".").length });
});

app.post("/verify", async (c) => {
  const input = await c.req.json() as Record<string, unknown>;
  const token = typeof input.token === "string" ? input.token : "";
  try {
    const payload = await checkToken(token, SECRET);
    return c.json({ ok: true, payload, sub: payload.sub });
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return c.json({ ok: false, error: msg }, 401);
  }
});

app.post("/sign-verify-cycle", async (c) => {
  const input = await c.req.json() as Record<string, unknown>;
  const sub = typeof input.sub === "string" ? input.sub : "cycle-user";
  const token = await makeToken({ sub, iss: "flux-compat" }, SECRET);
  const payload = await checkToken(token, SECRET);
  return c.json({ ok: true, sub_matches: payload.sub === sub, token_length: token.length });
});

app.post("/verify-bad", async (_c) => {
  // Construct a token with a clearly wrong signature
  const header = toBase64url(new TextEncoder().encode(JSON.stringify({ alg: "HS256", typ: "JWT" })));
  const body = toBase64url(new TextEncoder().encode(JSON.stringify({ sub: "attacker" })));
  const badToken = header + "." + body + ".INVALIDSIGNATURE";
  try {
    await checkToken(badToken, SECRET);
    return _c.json({ ok: false, error: "expected verification failure" }, 500);
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    return _c.json({ ok: true, caught: true, error: msg });
  }
});

app.post("/verify-expired", async (c) => {
  // Build a token with exp in the past
  const header = toBase64url(new TextEncoder().encode(JSON.stringify({ alg: "HS256", typ: "JWT" })));
  const pastPayload = { sub: "expired", iat: 1000000, exp: 1000001 };
  const body = toBase64url(new TextEncoder().encode(JSON.stringify(pastPayload)));
  const signingInput = new TextEncoder().encode(header + "." + body);
  const key = await getHmacKey(SECRET, "sign");
  const sigBuf = await crypto.subtle.sign("HMAC", key, signingInput);
  const expiredToken = header + "." + body + "." + toBase64url(sigBuf);
  try {
    await checkToken(expiredToken, SECRET);
    return c.json({ ok: false, error: "expected expiry error" }, 500);
  } catch (_e) {
    return c.json({ ok: true, caught: true, expired: true });
  }
});

app.get("/jwks", async (c) => {
  const keyPair = await crypto.subtle.generateKey(
    {
      name: "RSASSA-PKCS1-v1_5",
      modulusLength: 2048,
      publicExponent: new Uint8Array([1, 0, 1]),
      hash: "SHA-256",
    },
    true,
    ["sign", "verify"],
  );
  const jwk = await crypto.subtle.exportKey("jwk", keyPair.publicKey);
  return c.json({ keys: [{ ...jwk, kid: "flux-rs256-key", use: "sig", alg: "RS256" }] });
});

app.get("/digest", async (c) => {
  const input = "flux-determinism-test";
  const hashBuf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(input));
  const hashArr = Array.from(new Uint8Array(hashBuf));
  const hex = hashArr.map((b) => b.toString(16).padStart(2, "0")).join("");
  return c.json({ ok: true, input, hex });
});

app.post("/derive-key", async (c) => {
  const body = await c.req.json() as Record<string, unknown>;
  const password = typeof body.password === "string" ? body.password : "default";
  const salt = typeof body.salt === "string" ? body.salt : "default-salt";
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
  const hexArr = Array.from(new Uint8Array(bits));
  const hex = hexArr.map((b) => b.toString(16).padStart(2, "0")).join("");
  return c.json({ ok: true, derived_bits_length: bits.byteLength * 8, hex });
});

Deno.serve(app.fetch);
