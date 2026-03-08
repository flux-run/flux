export default async function (req, ctx) {
  return new Response(JSON.stringify({ message: "Hello from test function!" }), {
    headers: { "Content-Type": "application/json" },
  });
}
