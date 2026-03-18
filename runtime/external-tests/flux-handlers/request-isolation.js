let counter = 0;
let state = { seen: 0 };

Deno.serve((_req) => {
  const url = new URL(_req.url);

  if (url.pathname === "/counter") {
    counter += 1;
    return Response.json({ counter });
  }

  if (url.pathname === "/object-id") {
    state.seen += 1;
    return Response.json({ seen: state.seen });
  }

  return new Response(JSON.stringify({ error: "not found" }), {
    status: 404,
    headers: { "content-type": "application/json" },
  });
});