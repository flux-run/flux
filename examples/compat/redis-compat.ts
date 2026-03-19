// @ts-nocheck
// Compat test: node-redis via Flux.redis
import { Hono } from "npm:hono";
import redis from "flux:redis";

const app = new Hono();

function getClient() {
  return redis.createClient({ url: Deno.env.get("REDIS_URL") ?? "redis://localhost:6379" });
}

// GET / — smoke test (no Redis required)
app.get("/", (c) => c.json({ library: "node-redis", ok: true }));

// GET /ping — PING the Redis server
app.get("/ping", async (c) => {
  const client = getClient();
  await client.connect();
  const pong = await client.ping();
  await client.disconnect();
  return c.json({ ok: true, pong });
});

// POST /set-get — SET then GET a key
app.post("/set-get", async (c) => {
  const { key, value } = await c.req.json();
  const client = getClient();
  await client.connect();
  await client.set(`flux:compat:${key}`, value, { EX: 60 });
  const retrieved = await client.get(`flux:compat:${key}`);
  await client.del(`flux:compat:${key}`);
  await client.disconnect();
  return c.json({ ok: true, stored: value, retrieved });
});

// POST /incr — INCR a counter
app.post("/incr", async (c) => {
  const client = getClient();
  await client.connect();
  const counterKey = "flux:compat:counter";
  await client.del(counterKey);
  const v1 = await client.incr(counterKey);
  const v2 = await client.incr(counterKey);
  const v3 = await client.incrBy(counterKey, 10);
  await client.del(counterKey);
  await client.disconnect();
  return c.json({ ok: true, v1, v2, v3 });
});

// POST /hash — HSET / HGETALL
app.post("/hash", async (c) => {
  const { field, value } = await c.req.json();
  const client = getClient();
  await client.connect();
  const hashKey = "flux:compat:hash";
  await client.hSet(hashKey, field, value);
  const all = await client.hGetAll(hashKey);
  await client.del(hashKey);
  await client.disconnect();
  return c.json({ ok: true, all });
});

Deno.serve(app.fetch);
