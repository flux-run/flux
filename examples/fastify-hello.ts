// @ts-nocheck
import Fastify from "npm:fastify";

const fastify = Fastify();

fastify.get("/", async (request, reply) => {
  return "hello from fastify on flux";
});

fastify.get("/app-health", async (request, reply) => {
  return { ok: true };
});

fastify.post("/data", async (request, reply) => {
  return { received: request.body };
});

// Shimming Fastify to work with Deno.serve
Deno.serve(async (req) => {
  await fastify.ready();
  const res = await fastify.inject({
    method: req.method,
    url: new URL(req.url).pathname,
    headers: Object.fromEntries(req.headers.entries()),
    payload: await req.text(),
  });
  return new Response(res.payload, {
    status: res.statusCode,
    headers: res.headers,
  });
});
