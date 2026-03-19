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

// ── Failure cases ──────────────────────────────────────────────────────────

// GET /redis-missing — GET on a nonexistent key returns null
app.get("/redis-missing", async (c) => {
  const client = getClient();
  await client.connect();
  const missingKey = `flux:compat:doesnotexist:${Date.now()}`;
  const value = await client.get(missingKey);
  await client.disconnect();
  return c.json({ ok: true, value, is_null: value === null });
});

// POST /redis-ttl-expired — SET with 1ms, verify key expires
app.post("/redis-ttl-check", async (c) => {
  const client = getClient();
  await client.connect();
  const key = `flux:compat:ttl:${Date.now()}`;
  // Set with 1s TTL; get TTL immediately and verify it's set
  await client.set(key, "ephemeral", { EX: 1 });
  const ttl = await client.ttl(key);
  await client.del(key);
  await client.disconnect();
  return c.json({ ok: true, ttl_set: ttl > 0, ttl });
});

// GET /redis-type-ops — string, list, set operations
app.get("/redis-type-ops", async (c) => {
  const client = getClient();
  await client.connect();
  const listKey = "flux:compat:list";
  const setKey = "flux:compat:set";
  try {
    // List
    await client.del(listKey);
    await client.rPush(listKey, ["a", "b", "c"]);
    const listLen = await client.lLen(listKey);
    const listItems = await client.lRange(listKey, 0, -1);

    // Set
    await client.del(setKey);
    await client.sAdd(setKey, ["x", "y", "z"]);
    const setCard = await client.sCard(setKey);
    const isMember = await client.sIsMember(setKey, "y");

    return c.json({
      ok: true,
      list: { len: listLen, items: listItems },
      set: { card: setCard, y_is_member: isMember },
    });
  } finally {
    await client.del(listKey);
    await client.del(setKey);
    await client.disconnect();
  }
});

Deno.serve(app.fetch);
