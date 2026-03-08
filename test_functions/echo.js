// echo.js - Echoes back the request payload
export default async function (req, ctx) {
  const body = req.body ?? {};
  return new Response(JSON.stringify({
    message: "Echo from Fluxbase!",
    received: body,
    timestamp: new Date().toISOString(),
  }), {
    headers: { "Content-Type": "application/json" },
  });
}
