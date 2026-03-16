/**
 * web-apis.js — Flux integration test handler
 *
 * Tests built-in Web APIs available inside the Deno V8 isolate.
 *
 * Supported by flux-runtime:
 *   crypto.randomUUID, Date, URL / URLSearchParams
 *
 * NOT available in the flux-runtime isolate (excluded from tests):
 *   TextEncoder / TextDecoder  — not polyfilled
 *   btoa / atob                — not polyfilled
 *   structuredClone            — not polyfilled
 *   setTimeout with real delay — patched by replay system
 *
 * Routes:
 *   GET /web/uuid        – crypto.randomUUID()
 *   GET /web/date        – Date.now() and new Date().toISOString()
 *   GET /web/url         – URL parsing + search params
 *   GET /web/url-build   – constructing a URL from parts
 *   GET /web/math        – Math built-ins (random, floor, ceil, abs, min, max)
 *   GET /web/json        – JSON.stringify / JSON.parse round-trip
 */

Deno.serve((req) => {
  const url = new URL(req.url);

  switch (url.pathname) {
    case "/web/uuid": {
      const id = crypto.randomUUID();
      // RFC-4122 v4 UUID pattern
      const valid = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
        .test(id);
      return Response.json({ id, valid });
    }

    case "/web/date": {
      const now = Date.now();
      return Response.json({ timestamp: now, iso: new Date(now).toISOString() });
    }

    case "/web/url": {
      const parsed = new URL("https://example.com/path?foo=1&bar=2");
      return Response.json({
        host:     parsed.host,
        pathname: parsed.pathname,
        foo:      parsed.searchParams.get("foo"),
        bar:      parsed.searchParams.get("bar"),
      });
    }

    case "/web/url-build": {
      // Use full URL string — property setters on URL objects are not
      // available in the flux-runtime isolate's URL implementation.
      const u = new URL("https://api.example.com/v1/users?page=2&limit=10");
      return Response.json({ href: u.href, page: u.searchParams.get("page"), path: u.pathname });
    }

    case "/web/math": {
      const r = Math.random();
      return Response.json({
        random_in_range: r >= 0 && r < 1,
        floor:  Math.floor(3.9),
        ceil:   Math.ceil(3.1),
        abs:    Math.abs(-7),
        min:    Math.min(5, 3, 8),
        max:    Math.max(5, 3, 8),
        round:  Math.round(2.5),
        pow:    Math.pow(2, 10),
      });
    }

    case "/web/json": {
      const original = { a: 1, b: [1, 2, 3], c: { nested: true }, d: null };
      const json     = JSON.stringify(original);
      const parsed2  = JSON.parse(json);
      return Response.json({
        json,
        match: parsed2.a === 1 && parsed2.b.length === 3 && parsed2.c.nested === true && parsed2.d === null,
      });
    }

    default:
      return new Response(JSON.stringify({ error: "not found" }), {
        status: 404,
        headers: { "content-type": "application/json" },
      });
  }
});
