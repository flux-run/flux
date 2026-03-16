/**
 * error-handling.js — Flux integration test handler
 *
 * Tests how thrown errors, rejected Promises, and explicit error responses
 * behave inside the Deno isolate.
 *
 * Routes:
 *   GET  /error/not-found          – explicit 404 response
 *   GET  /error/bad-request        – explicit 400 response
 *   GET  /error/sync-throw         – throws synchronously (expect 500)
 *   GET  /error/async-reject       – returned rejected promise (expect 500)
 *   POST /error/missing-field      – validates that a required field exists
 */

Deno.serve(async (req) => {
  const url = new URL(req.url);

  switch (url.pathname) {
    case "/error/not-found":
      return new Response(JSON.stringify({ error: "resource not found" }), {
        status: 404,
        headers: { "content-type": "application/json" },
      });

    case "/error/bad-request":
      return new Response(JSON.stringify({ error: "bad request" }), {
        status: 400,
        headers: { "content-type": "application/json" },
      });

    case "/error/sync-throw":
      throw new Error("intentional synchronous throw");

    case "/error/async-reject":
      return Promise.reject(new Error("intentional async rejection"));

    case "/error/missing-field": {
      let body;
      try {
        body = await req.json();
      } catch {
        return new Response(JSON.stringify({ error: "invalid JSON" }), {
          status: 400,
          headers: { "content-type": "application/json" },
        });
      }
      if (!body || typeof body.name !== "string" || !body.name.trim()) {
        return new Response(JSON.stringify({ error: "name is required" }), {
          status: 422,
          headers: { "content-type": "application/json" },
        });
      }
      return Response.json({ greeting: `Hello, ${body.name}` });
    }

    default:
      return new Response(JSON.stringify({ error: "not found" }), {
        status: 404,
        headers: { "content-type": "application/json" },
      });
  }
});
