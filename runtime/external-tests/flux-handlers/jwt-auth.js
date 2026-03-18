const JWKS_URL = Deno.env.get("JWKS_URL") ?? "http://127.0.0.1:9020/.well-known/jwks.json";
const JWT_ISSUER = Deno.env.get("JWT_ISSUER") ?? "http://127.0.0.1:9020/";
const JWT_AUDIENCE = Deno.env.get("JWT_AUDIENCE") ?? "flux-api";

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

class JwksUnavailableError extends Error {
  constructor(message) {
    super(message);
    this.name = "JwksUnavailableError";
  }
}

function base64UrlToBytes(input) {
  const normalized = String(input).replace(/-/g, "+").replace(/_/g, "/");
  const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, "=");
  return Uint8Array.from(atob(padded), (char) => char.charCodeAt(0));
}

function parseJsonSegment(segment) {
  return JSON.parse(textDecoder.decode(base64UrlToBytes(segment)));
}

function audienceMatches(value) {
  if (Array.isArray(value)) {
    return value.includes(JWT_AUDIENCE);
  }
  return value === JWT_AUDIENCE;
}

function validateClaims(payload) {
  const now = Math.floor(Date.now() / 1000);
  if (typeof payload.exp !== "number" || payload.exp <= now) {
    throw new Error("token expired");
  }
  if (payload.nbf != null && (typeof payload.nbf !== "number" || payload.nbf > now)) {
    throw new Error("token not active");
  }
  if (payload.iss !== JWT_ISSUER) {
    throw new Error("issuer mismatch");
  }
  if (!audienceMatches(payload.aud)) {
    throw new Error("audience mismatch");
  }
}

async function fetchJwks(bypass) {
  let response;
  try {
    response = await fetch(JWKS_URL, {
      headers: bypass ? { "cache-control": "no-cache" } : undefined,
    });
  } catch (error) {
    throw new JwksUnavailableError(error?.message ?? String(error));
  }
  if (!response.ok) {
    throw new JwksUnavailableError(`jwks fetch failed: ${response.status}`);
  }
  return response.json();
}

async function verifyJwt(token, bypass) {
  const parts = String(token).split(".");
  if (parts.length !== 3) {
    throw new Error("malformed jwt");
  }

  const [encodedHeader, encodedPayload, encodedSignature] = parts;
  const header = parseJsonSegment(encodedHeader);
  const payload = parseJsonSegment(encodedPayload);

  if (header.alg !== "RS256") {
    throw new Error("unsupported jwt algorithm");
  }
  if (typeof header.kid !== "string" || header.kid.length === 0) {
    throw new Error("jwt header missing kid");
  }

  const jwks = await fetchJwks(bypass);
  const jwk = Array.isArray(jwks?.keys)
    ? jwks.keys.find((candidate) => candidate && candidate.kid === header.kid)
    : null;
  if (!jwk) {
    throw new Error("signing key not found");
  }

  const key = await crypto.subtle.importKey(
    "jwk",
    jwk,
    { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" },
    false,
    ["verify"],
  );
  const verified = await crypto.subtle.verify(
    { name: "RSASSA-PKCS1-v1_5" },
    key,
    base64UrlToBytes(encodedSignature),
    textEncoder.encode(`${encodedHeader}.${encodedPayload}`),
  );

  if (!verified) {
    throw new Error("invalid signature");
  }

  validateClaims(payload);
  return payload;
}

async function authenticate(req, bypass) {
  const authorization = req.headers.get("authorization");
  if (!authorization?.startsWith("Bearer ")) {
    return {
      response: Response.json({ error: "missing bearer token", bypass }, { status: 401 }),
    };
  }

  try {
    return {
      payload: await verifyJwt(authorization.slice(7), bypass),
    };
  } catch (error) {
    const message = error?.message ?? String(error);
    const status = error?.name === "JwksUnavailableError" ? 503 : 401;
    return {
      response: Response.json({ error: message, bypass }, { status }),
    };
  }
}

Deno.serve(async (req) => {
  const url = new URL(req.url);
  const bypass = url.pathname === "/protected-bypass";

  switch (url.pathname) {
    case "/public":
      return Response.json({ ok: true, protected: false });

    case "/protected":
    case "/protected-bypass": {
      const auth = await authenticate(req, bypass);
      if (auth.response) {
        return auth.response;
      }

      return Response.json({
        ok: true,
        protected: true,
        bypass,
        sub: auth.payload.sub ?? null,
        iss: auth.payload.iss ?? null,
        aud: auth.payload.aud ?? null,
        scope: auth.payload.scope ?? null,
      });
    }

    default:
      return Response.json({ error: "not found" }, { status: 404 });
  }
});