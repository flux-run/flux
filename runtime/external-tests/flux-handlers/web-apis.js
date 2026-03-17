/**
 * web-apis.js — Flux integration test handler
 *
 * Tests Web APIs available inside the Flux runtime.
 *
 * Supported by flux-runtime:
 *   crypto.randomUUID, Date, URL / URLSearchParams, Headers,
 *   Request / Response, TextEncoder / TextDecoder
 *
 * Routes:
 *   GET /web/uuid        – crypto.randomUUID()
 *   GET /web/date        – Date.now() and new Date().toISOString()
 *   GET /web/url         – URL parsing + search params
 *   GET /web/url-build   – constructing a URL from parts
 *   GET /web/url-search-params – URLSearchParams round-trip semantics
 *   POST /web/headers    – request/response header semantics
 *   POST /web/request-info – incoming Request inspection
 *   GET /web/request-construct – Request constructor semantics
 *   GET /web/response    – Response constructor semantics
 *   GET /web/text-encoding – TextEncoder / TextDecoder round-trip
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

    case "/web/url-search-params": {
      const params = new URLSearchParams("tag=alpha&tag=beta&space=hello+world");
      params.append("extra", "42");
      params.set("single", "value");
      return Response.json({
        tags: params.getAll("tag"),
        space: params.get("space"),
        extra: params.get("extra"),
        hasExtra: params.has("extra"),
        single: params.get("single"),
        text: params.toString(),
      });
    }

    case "/web/headers": {
      const headers = new Headers();
      headers.set("content-type", "application/json");
      headers.set("x-one", "alpha");
      headers.append("x-one", "beta");
      headers.set("x-two", "gamma");
      return new Response(JSON.stringify({
        inbound: req.headers.get("x-custom"),
        caseInsensitive: req.headers.get("X-CUSTOM"),
        hasJson: req.headers.get("content-type") === "application/json",
      }), {
        status: 202,
        headers,
      });
    }

    case "/web/request-info": {
      return req.text().then((body) => Response.json({
        isRequest: req instanceof Request,
        method: req.method,
        pathname: url.pathname,
        query: url.searchParams.get("foo"),
        header: req.headers.get("x-custom"),
        body,
      }));
    }

    case "/web/request-construct": {
      const built = new Request("https://api.example.com/items?foo=bar", {
        method: "POST",
        headers: new Headers([
          ["content-type", "text/plain"],
          ["x-extra", "demo"],
        ]),
        body: "payload",
      });
      return built.text().then((body) => Response.json({
        isRequest: built instanceof Request,
        method: built.method,
        host: new URL(built.url).host,
        query: new URL(built.url).searchParams.get("foo"),
        contentType: built.headers.get("content-type"),
        extra: built.headers.get("x-extra"),
        body,
      }));
    }

    case "/web/response": {
      return new Response("created", {
        status: 201,
        headers: {
          "content-type": "text/plain",
          "x-response": "ok",
        },
      });
    }

    case "/web/text-encoding": {
      const source = "Flux 日本語";
      const encoded = new TextEncoder().encode(source);
      const decoded = new TextDecoder().decode(encoded);
      return Response.json({
        decoded,
        byteLength: encoded.length,
        prefix: Array.from(encoded.slice(0, 4)),
      });
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
