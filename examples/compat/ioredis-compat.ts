// @ts-nocheck
// Compat test: ioredis client (most widely used Redis client)
import { Hono } from "npm:hono";
import Redis from "npm:ioredis";

const app = new Hono();

function getClient() {
  return new Redis(Deno.env.get("REDIS_URL") ?? "redis://localhost:6379");
}

// GET / — smoke test (no Redis required)
app.get("/", (c) => c.json({ library: "ioredis", ok: true }));

// GET /ping — PING the server
app.get("/ping", async (c) => {
  const client = getClient();
  try {
    const pong = await client.ping();
    return c.json({ ok: true, pong });
  } finally {
    client.disconnect();
  }
});

// POST /set-get — SET + GET + DEL
app.post("/set-get", async (c) => {
  const { key, value } = await c.req.json();
  const client = getClient();
  try {
    await client.set(`flux:ioredis:${key}`, value, "EX", 60);
    const retrieved = await client.get(`flux:ioredis:${key}`);
    await client.del(`flux:ioredis:${key}`);
    return c.json({ ok: true, stored: value, retrieved });
  } finally {
    client.disconnect();
  }
});

// POST /incr — INCR and INCRBY
app.post("/incr", async (c) => {
  const client = getClient();
  const counterKey = "flux:ioredis:counter";
  try {
    await client.del(counterKey);
    const v1 = await client.incr(counterKey);
    const v2 = await client.incr(counterKey);
    const v3 = await client.incrby(counterKey, 10);
    await client.del(counterKey);
    return c.json({ ok: true, v1, v2, v3 });
  } finally {
    client.disconnect();
  }
});

// POST /hash — HSET + HGETALL
app.post("/hash", async (c) => {
  const { field, value } = await c.req.json();
  const client = getClient();
  const hashKey = "flux:ioredis:hash";
  try {
    await client.hset(hashKey, field, value);
    const all = await client.hgetall(hashKey);
    await client.del(hashKey);
    return c.json({ ok: true, all });
  } finally {
    client.disconnect();
  }
});

// POST /pipeline — ioredis pipeline (batch commands)
app.post("/pipeline", async (c) => {
  const client = getClient();
  const key = "flux:ioredis:pipeline";
  try {
    await client.del(key);
    const pipeline = client.pipeline();
    pipeline.set(key, "flux-pipeline", "EX", 60);
    pipeline.get(key);
    pipeline.del(key);
    const results = await pipeline.exec();
    // results is [[null, 'OK'], [null, 'flux-pipeline'], [null, 1]]
    const getValue = (results?.[1] as any)?.[1];
    return c.json({ ok: true, pipeline_value: getValue });
  } finally {
    client.disconnect();
  }
});

Deno.serve(app.fetch);
