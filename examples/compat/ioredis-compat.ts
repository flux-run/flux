// @ts-nocheck
// Compat test: ioredis — exhaustive coverage
// Tests: PING, strings (SET/GET/DEL/EXPIRE/TTL/EXISTS), counters (INCR/DECR),
//        hashes, lists, sets, sorted sets, pipeline, multi/exec, expiry, patterns
import { Hono } from "npm:hono";
import Redis from "npm:ioredis";

const app = new Hono();
const KP = "flux:ioredis"; // key prefix

function getClient() {
  return new Redis(Deno.env.get("REDIS_URL") ?? "redis://localhost:6379");
}

// ── Smoke ─────────────────────────────────────────────────────────────────

app.get("/", (c) => c.json({ library: "ioredis", ok: true }));

// ── Connection ────────────────────────────────────────────────────────────

app.get("/ping", async (c) => {
  const r = getClient();
  try { return c.json({ ok: true, pong: await r.ping() }); }
  finally { r.disconnect(); }
});

app.get("/info", async (c) => {
  const r = getClient();
  try {
    const info = await r.info("server");
    return c.json({ ok: true, has_version: info.includes("redis_version") });
  } finally { r.disconnect(); }
});

// ── Strings ───────────────────────────────────────────────────────────────

app.post("/set-get", async (c) => {
  const { key, value } = await c.req.json();
  const r = getClient();
  try {
    await r.set(`${KP}:${key}`, value, "EX", 60);
    const retrieved = await r.get(`${KP}:${key}`);
    await r.del(`${KP}:${key}`);
    return c.json({ ok: true, match: retrieved === value });
  } finally { r.disconnect(); }
});

app.post("/setnx", async (c) => {
  const k = `${KP}:setnx:${Date.now()}`;
  const r = getClient();
  try {
    const first = await r.setnx(k, "first");
    const second = await r.setnx(k, "second");
    const val = await r.get(k);
    await r.del(k);
    return c.json({ ok: true, first_set: first === 1, second_set: second === 0, value: val });
  } finally { r.disconnect(); }
});

app.post("/getset", async (c) => {
  const k = `${KP}:getset`;
  const r = getClient();
  try {
    await r.set(k, "old");
    const old = await r.getset(k, "new");
    const current = await r.get(k);
    await r.del(k);
    return c.json({ ok: true, old, current });
  } finally { r.disconnect(); }
});

app.post("/mset-mget", async (c) => {
  const r = getClient();
  const keys = [`${KP}:m1`, `${KP}:m2`, `${KP}:m3`];
  try {
    await r.mset(keys[0], "a", keys[1], "b", keys[2], "c");
    const vals = await r.mget(...keys);
    await r.del(...keys);
    return c.json({ ok: true, values: vals });
  } finally { r.disconnect(); }
});

app.post("/append", async (c) => {
  const k = `${KP}:append`;
  const r = getClient();
  try {
    await r.del(k);
    await r.append(k, "hello");
    await r.append(k, " world");
    const val = await r.get(k);
    await r.del(k);
    return c.json({ ok: true, value: val });
  } finally { r.disconnect(); }
});

// ── Expiry ────────────────────────────────────────────────────────────────

app.post("/expiry", async (c) => {
  const k = `${KP}:expiry`;
  const r = getClient();
  try {
    await r.set(k, "temp", "EX", 10);
    const ttl = await r.ttl(k);
    const exists = await r.exists(k);
    await r.del(k);
    return c.json({ ok: true, ttl_positive: ttl > 0, exists: exists === 1 });
  } finally { r.disconnect(); }
});

app.post("/persist", async (c) => {
  const k = `${KP}:persist`;
  const r = getClient();
  try {
    await r.set(k, "data", "EX", 10);
    await r.persist(k);
    const ttl = await r.ttl(k);
    await r.del(k);
    return c.json({ ok: true, ttl_is_persistent: ttl === -1 });
  } finally { r.disconnect(); }
});

app.post("/pexpiry", async (c) => {
  const k = `${KP}:pex`;
  const r = getClient();
  try {
    await r.psetex(k, 500, "milliseconds");
    const pttl = await r.pttl(k);
    await r.del(k);
    return c.json({ ok: true, pttl_positive: pttl > 0 });
  } finally { r.disconnect(); }
});

// ── Counters ──────────────────────────────────────────────────────────────

app.post("/incr", async (c) => {
  const k = `${KP}:counter`;
  const r = getClient();
  try {
    await r.del(k);
    const v1 = await r.incr(k);
    const v2 = await r.incrby(k, 5);
    const v3 = await r.incrbyfloat(k, 0.5);
    const v4 = await r.decr(k);
    const v5 = await r.decrby(k, 2);
    await r.del(k);
    return c.json({ ok: true, v1, v2, after_float: String(v3), v4, v5 });
  } finally { r.disconnect(); }
});

// ── Hashes ────────────────────────────────────────────────────────────────

app.post("/hash", async (c) => {
  const k = `${KP}:hash`;
  const r = getClient();
  try {
    await r.del(k);
    await r.hset(k, "name", "Flux", "version", "1", "active", "true");
    const name = await r.hget(k, "name");
    const all = await r.hgetall(k);
    const keys = await r.hkeys(k);
    const vals = await r.hvals(k);
    const len = await r.hlen(k);
    await r.hdel(k, "active");
    const exists = await r.hexists(k, "active");
    await r.del(k);
    return c.json({ ok: true, name, all, keys, vals_count: vals.length, len, active_removed: exists === 0 });
  } finally { r.disconnect(); }
});

app.post("/hincrby", async (c) => {
  const k = `${KP}:hincr`;
  const r = getClient();
  try {
    await r.del(k);
    await r.hset(k, "count", "0");
    const v1 = await r.hincrby(k, "count", 5);
    const v2 = await r.hincrbyfloat(k, "count", 1.5);
    await r.del(k);
    return c.json({ ok: true, v1, v2: String(v2) });
  } finally { r.disconnect(); }
});

// ── Lists ─────────────────────────────────────────────────────────────────

app.post("/list", async (c) => {
  const k = `${KP}:list`;
  const r = getClient();
  try {
    await r.del(k);
    await r.rpush(k, "a", "b", "c");
    await r.lpush(k, "z");
    const len = await r.llen(k);
    const all = await r.lrange(k, 0, -1);
    const first = await r.lindex(k, 0);
    const popped = await r.rpop(k);
    const lpop = await r.lpop(k);
    await r.del(k);
    return c.json({ ok: true, len, all, first, popped, lpop });
  } finally { r.disconnect(); }
});

app.post("/list-trim", async (c) => {
  const k = `${KP}:listtrim`;
  const r = getClient();
  try {
    await r.del(k);
    await r.rpush(k, "1", "2", "3", "4", "5");
    await r.ltrim(k, 1, 3); // keep indices 1-3
    const after = await r.lrange(k, 0, -1);
    await r.del(k);
    return c.json({ ok: true, after });
  } finally { r.disconnect(); }
});

// ── Sets ──────────────────────────────────────────────────────────────────

app.post("/set", async (c) => {
  const k = `${KP}:set`;
  const r = getClient();
  try {
    await r.del(k);
    await r.sadd(k, "a", "b", "c", "a"); // "a" is duplicate
    const len = await r.scard(k);
    const isMember = await r.sismember(k, "a");
    const notMember = await r.sismember(k, "z");
    const members = await r.smembers(k);
    await r.srem(k, "b");
    const afterRemove = await r.scard(k);
    await r.del(k);
    return c.json({ ok: true, len, isMember: isMember === 1, notMember: notMember === 0, members_count: members.length, afterRemove });
  } finally { r.disconnect(); }
});

app.post("/set-ops", async (c) => {
  const k1 = `${KP}:set1`, k2 = `${KP}:set2`;
  const r = getClient();
  try {
    await r.del(k1, k2);
    await r.sadd(k1, "a", "b", "c");
    await r.sadd(k2, "b", "c", "d");
    const inter = await r.sinter(k1, k2);
    const union = await r.sunion(k1, k2);
    const diff = await r.sdiff(k1, k2);
    await r.del(k1, k2);
    return c.json({ ok: true, inter: inter.sort(), union: union.sort(), diff: diff.sort() });
  } finally { r.disconnect(); }
});

// ── Sorted sets ────────────────────────────────────────────────────────────

app.post("/zset", async (c) => {
  const k = `${KP}:zset`;
  const r = getClient();
  try {
    await r.del(k);
    await r.zadd(k, 10, "alice", 20, "bob", 5, "charlie");
    const len = await r.zcard(k);
    const rank = await r.zrank(k, "alice");
    const score = await r.zscore(k, "bob");
    const range = await r.zrange(k, 0, -1, "WITHSCORES");
    const top = await r.zrevrange(k, 0, 1);
    await r.zrem(k, "charlie");
    const afterRemove = await r.zcard(k);
    await r.del(k);
    return c.json({ ok: true, len, rank, score, range, top, afterRemove });
  } finally { r.disconnect(); }
});

app.post("/zrangebyscore", async (c) => {
  const k = `${KP}:zbyscore`;
  const r = getClient();
  try {
    await r.del(k);
    await r.zadd(k, 1, "a", 5, "b", 10, "c", 15, "d");
    const range = await r.zrangebyscore(k, 4, 12);
    const count = await r.zcount(k, 4, 12);
    await r.del(k);
    return c.json({ ok: true, range, count });
  } finally { r.disconnect(); }
});

// ── Pipeline ──────────────────────────────────────────────────────────────

app.post("/pipeline", async (c) => {
  const k = `${KP}:pipe`;
  const r = getClient();
  try {
    await r.del(k);
    const pipe = r.pipeline();
    pipe.set(k, "pipe-value", "EX", 60);
    pipe.get(k);
    pipe.incr(`${k}:count`);
    pipe.incr(`${k}:count`);
    pipe.get(`${k}:count`);
    pipe.del(k, `${k}:count`);
    const results = await pipe.exec();
    return c.json({
      ok: true,
      set_ok: results?.[0]?.[1] === "OK",
      get_value: results?.[1]?.[1],
      count_value: results?.[4]?.[1],
    });
  } finally { r.disconnect(); }
});

// ── Multi/exec (atomic transaction) ───────────────────────────────────────

app.post("/multi-exec", async (c) => {
  const k = `${KP}:multi`;
  const r = getClient();
  try {
    await r.del(k);
    const results = await r
      .multi()
      .set(k, "atomic")
      .get(k)
      .del(k)
      .exec();
    return c.json({ ok: true, set_ok: results?.[0]?.[1] === "OK", get_val: results?.[1]?.[1] });
  } finally { r.disconnect(); }
});

// ── Key operations ────────────────────────────────────────────────────────

app.get("/keys-scan", async (c) => {
  const r = getClient();
  try {
    const prefix = `${KP}:scan:${Date.now()}`;
    await r.mset(`${prefix}:a`, "1", `${prefix}:b`, "2", `${prefix}:c`, "3");
    const [, keys] = await r.scan(0, "MATCH", `${prefix}:*`, "COUNT", 100);
    await r.del(...keys);
    return c.json({ ok: true, found: keys.length });
  } finally { r.disconnect(); }
});

app.get("/type-check", async (c) => {
  const r = getClient();
  const ks = { str: `${KP}:type:str`, lst: `${KP}:type:lst`, st: `${KP}:type:set` };
  try {
    await r.set(ks.str, "hello");
    await r.rpush(ks.lst, "a");
    await r.sadd(ks.st, "x");
    const [t1, t2, t3] = await Promise.all([r.type(ks.str), r.type(ks.lst), r.type(ks.st)]);
    await r.del(ks.str, ks.lst, ks.st);
    return c.json({ ok: true, str_type: t1, list_type: t2, set_type: t3 });
  } finally { r.disconnect(); }
});

// ── Concurrency ────────────────────────────────────────────────────────────

app.get("/concurrent", async (c) => {
  const r = getClient();
  const base = `${KP}:conc:${Date.now()}`;
  try {
    const ops = Array.from({ length: 5 }, (_, i) =>
      r.set(`${base}:${i}`, String(i), "EX", 30),
    );
    const sets = await Promise.all(ops);
    const gets = await Promise.all(Array.from({ length: 5 }, (_, i) => r.get(`${base}:${i}`)));
    await r.del(...Array.from({ length: 5 }, (_, i) => `${base}:${i}`));
    return c.json({ ok: true, all_set: sets.every((s) => s === "OK"), values: gets });
  } finally { r.disconnect(); }
});

Deno.serve(app.fetch);
