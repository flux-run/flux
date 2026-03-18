Deno.serve(async (req) => {
  const url = new URL(req.url);
  const bypass = url.pathname === "/jwks-bypass";

  try {
    const response = await fetch("http://127.0.0.1:9020/.well-known/jwks.json", {
      headers: bypass ? { "cache-control": "no-cache" } : undefined,
    });
    const json = await response.json();
    const keyCount = Array.isArray(json?.keys) ? json.keys.length : 0;

    return Response.json({
      keys: keyCount,
      bypass,
    });
  } catch (error) {
    return new Response(JSON.stringify({
      error: error?.message ?? String(error),
      bypass,
    }), {
      status: 502,
      headers: { "content-type": "application/json" },
    });
  }
});