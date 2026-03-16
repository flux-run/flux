/**
 * json-types.js — Flux integration test handler
 *
 * Tests that several JSON value types round-trip correctly through the runtime.
 *
 * Routes:
 *   GET /types/null
 *   GET /types/bool
 *   GET /types/number
 *   GET /types/string
 *   GET /types/array
 *   GET /types/nested
 *   GET /types/all     — returns all types at once
 */

const handlers = {
  "/types/null":   () => Response.json({ value: null }),
  "/types/bool":   () => Response.json({ value: true }),
  "/types/number": () => Response.json({ value: 42, float: 3.14 }),
  "/types/string": () => Response.json({ value: "hello flux" }),
  "/types/array":  () => Response.json({ value: [1, "two", true, null] }),
  "/types/nested": () =>
    Response.json({ outer: { inner: { deep: "yes" }, arr: [1, 2, 3] } }),
  "/types/all": () =>
    Response.json({
      null:    null,
      bool:    false,
      integer: -7,
      float:   1.5e-3,
      string:  "utf-8: 日本語 🎉",
      array:   [1, 2, 3],
      object:  { a: 1 },
    }),
};

Deno.serve((req) => {
  const url     = new URL(req.url);
  const handler = handlers[url.pathname];
  if (handler) return handler();
  return new Response(JSON.stringify({ error: "not found" }), {
    status: 404,
    headers: { "content-type": "application/json" },
  });
});
