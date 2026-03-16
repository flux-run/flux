/**
 * echo.js — Flux integration test handler
 *
 * Routes:
 *   POST /echo          – reflects the request body back as JSON
 *   POST /echo/upper    – returns string fields uppercased
 *   GET  /ping          – liveness probe
 */

Deno.serve(async (req) => {
  const url = new URL(req.url);

  // Liveness probe
  if (req.method === "GET" && url.pathname === "/ping") {
    return Response.json({ ok: true });
  }

  if (req.method === "POST" && url.pathname === "/echo") {
    let body;
    try {
      body = await req.json();
    } catch {
      return new Response(JSON.stringify({ error: "invalid JSON" }), {
        status: 400,
        headers: { "content-type": "application/json" },
      });
    }
    return Response.json(body);
  }

  if (req.method === "POST" && url.pathname === "/echo/upper") {
    let body;
    try {
      body = await req.json();
    } catch {
      return new Response(JSON.stringify({ error: "invalid JSON" }), {
        status: 400,
        headers: { "content-type": "application/json" },
      });
    }
    const result = {};
    for (const [k, v] of Object.entries(body)) {
      result[k] = typeof v === "string" ? v.toUpperCase() : v;
    }
    return Response.json(result);
  }

  return new Response(JSON.stringify({ error: "not found" }), {
    status: 404,
    headers: { "content-type": "application/json" },
  });
});
