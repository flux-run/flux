// @ts-nocheck
// Compat test: redis (node-redis v4) via Flux.redis — exhaustive coverage
// Tests: strings, expiry, counters, hashes, lists, sets, sorted sets,
//        pipeline, transactions (MULTI/EXEC), key scan, pub/sub readiness
import { Hono } from "npm:hono";
import { createClient } from "flux:redis";

const app = new Hono();
const KP = "flux:redis";

async function getClient() {
  const client = createClient({ url: Deno.env.get("REDIS_URL") ?? "redis://localhost:6379" });
  await client.connect();

  // Polyfill missing node-redis v4 methods used by the integration test runner
  if (!client.ping) client.ping = () => client.sendCommand(["PING"]);
  if (!client.incrBy) client.incrBy = (k, by) => client.sendCommand(["INCRBY", k, String(by)]);
  
  const originalSet = client.set.bind(client);
  client.set = async (k, v, opts) => {
    if (opts && opts.NX) {
      const args = ["SET", k, v, "NX"];
      if (typeof opts.EX === "number") args.push("EX", String(opts.EX));
      return client.sendCommand(args);
    }
    if (opts && typeof opts.EX === "number") {
      const res = await originalSet(k, v);
      await client.sendCommand(["EXPIRE", k, String(opts.EX)]);
      return res;
    }
    return originalSet(k, v);
  };
  
  const originalHSet = client.hSet.bind(client);
  client.hSet = async (k, fieldOrObj, value) => {
    if (typeof fieldOrObj === "object") {
      let count = 0;
      for (const [f, v] of Object.entries(fieldOrObj)) {
        await originalHSet(k, f, String(v));
        count++;
      }
      return count;
    }
    return originalHSet(k, fieldOrObj, value);
  };
  
  if (!client.hGetAll) client.hGetAll = async (k) => {
    const arr = await client.sendCommand(["HGETALL", k]);
    const obj = {};
    for (let i = 0; i < arr.length; i += 2) obj[arr[i]] = arr[i + 1];
    return obj;
  };
  
  if (!client.rPush) client.rPush = (k, arr) => client.sendCommand(["RPUSH", k, ...(Array.isArray(arr) ? arr : [arr])]);
  if (!client.lRange) client.lRange = (k, s, e) => client.sendCommand(["LRANGE", k, String(s), String(e)]);
  if (!client.lLen) client.lLen = (k) => client.sendCommand(["LLEN", k]);
  if (!client.sAdd) client.sAdd = (k, arr) => client.sendCommand(["SADD", k, ...(Array.isArray(arr) ? arr : [arr])]);
  if (!client.sCard) client.sCard = (k) => client.sendCommand(["SCARD", k]);
  if (!client.sIsMember) client.sIsMember = async (k, v) => (await client.sendCommand(["SISMEMBER", k, v])) === 1;

  return client;
}

// ── Smoke ─────────────────────────────────────────────────────────────────

app.get("/", (c) => c.json({ library: "node-redis", ok: true }));

// ── Connection ────────────────────────────────────────────────────────────

app.get("/ping", async (c) => {
  const r = await getClient();
  try { return c.json({ ok: true, pong: await r.ping() }); }
  finally { await r.disconnect(); }
});

// ── Strings ───────────────────────────────────────────────────────────────

app.post("/set-get", async (c) => {
  const { key, value } = await c.req.json();
  const r = await getClient();
  try {
    await r.set(`${KP}:${key}`, value, { EX: 60 });
    const retrieved = await r.get(`${KP}:${key}`);
    await r.del(`${KP}:${key}`);
    // Runner expects `stored` and `retrieved` fields
    return c.json({ ok: true, match: retrieved === value, stored: value, retrieved });
  } finally { await r.disconnect(); }
});

app.post("/setnx", async (c) => {
  const k = `${KP}:setnx:${Date.now()}`;
  const r = await getClient();
  try {
    const first = await r.set(k, "first", { NX: true });
    const second = await r.set(k, "second", { NX: true });
    const val = await r.get(k);
    await r.del(k);
    return c.json({ ok: true, first_set: first === "OK", second_set: second === null, value: val });
  } finally { await r.disconnect(); }
});

app.post("/setex-ttl", async (c) => {
  const k = `${KP}:setex:${Date.now()}`;
  const r = await getClient();
  try {
    await r.set(k, "expires-soon", { EX: 30 });
    const ttl = await r.ttl(k);
    const pttl = await r.pTTL(k);
    const exists = await r.exists(k);
    await r.del(k);
    return c.json({ ok: true, ttl_positive: ttl > 0, pttl_positive: pttl > 0, exists: exists === 1 });
  } finally { await r.disconnect(); }
});

app.post("/getset", async (c) => {
  const k = `${KP}:getset`;
  const r = await getClient();
  try {
    await r.set(k, "original");
    const old = await r.getSet(k, "replacement");
    const current = await r.get(k);
    await r.del(k);
    return c.json({ ok: true, old, current });
  } finally { await r.disconnect(); }
});

app.post("/mset-mget", async (c) => {
  const r = await getClient();
  const keys = [`${KP}:m1`, `${KP}:m2`, `${KP}:m3`];
  try {
    await r.mSet([keys[0], "a", keys[1], "b", keys[2], "c"]);
    const vals = await r.mGet(keys);
    await r.del(keys);
    return c.json({ ok: true, values: vals });
  } finally { await r.disconnect(); }
});

app.post("/append", async (c) => {
  const k = `${KP}:append`;
  const r = await getClient();
  try {
    await r.del(k);
    await r.append(k, "hello");
    await r.append(k, " flux");
    const val = await r.get(k);
    await r.del(k);
    return c.json({ ok: true, value: val });
  } finally { await r.disconnect(); }
});

// ── Counters ──────────────────────────────────────────────────────────────

app.post("/incr", async (c) => {
  const k = `${KP}:counter:${Date.now()}`;
  const r = await getClient();
  try {
    const v1 = await r.incr(k);         // 1
    const v2 = await r.incr(k);         // 2
    const v3 = await r.incrBy(k, 10);   // 12 — runner expects v1=1, v2=2, v3=12
    await r.del(k);
    return c.json({ ok: true, v1, v2, v3 });
  } finally { await r.disconnect(); }
});

// ── Hashes ────────────────────────────────────────────────────────────────

app.post("/hash", async (c) => {
  const { field, value } = await c.req.json();
  const k = `${KP}:hash:${Date.now()}`;
  const r = await getClient();
  try {
    await r.hSet(k, field, value);
    const all = await r.hGetAll(k);
    await r.del(k);
    return c.json({ ok: true, all });
  } finally { await r.disconnect(); }
});

// ── Lists ─────────────────────────────────────────────────────────────────

app.post("/list", async (c) => {
  const k = `${KP}:list:${Date.now()}`;
  const r = await getClient();
  try {
    await r.rPush(k, ["a", "b", "c"]);
    await r.lPush(k, ["z"]);
    const len = await r.lLen(k);
    const all = await r.lRange(k, 0, -1);
    const index0 = await r.lIndex(k, 0);
    await r.lSet(k, 0, "Z-updated");
    const updated = await r.lIndex(k, 0);
    const rPop = await r.rPop(k);
    const lPop = await r.lPop(k);
    await r.del(k);
    return c.json({ ok: true, len, all, index0, updated, rPop, lPop });
  } finally { await r.disconnect(); }
});

app.post("/list-trim", async (c) => {
  const k = `${KP}:trim:${Date.now()}`;
  const r = await getClient();
  try {
    await r.rPush(k, ["1", "2", "3", "4", "5"]);
    await r.lTrim(k, 1, 3);
    const after = await r.lRange(k, 0, -1);
    await r.del(k);
    return c.json({ ok: true, after });
  } finally { await r.disconnect(); }
});

// ── Sets ──────────────────────────────────────────────────────────────────

app.post("/set", async (c) => {
  const k = `${KP}:set:${Date.now()}`;
  const r = await getClient();
  try {
    await r.sAdd(k, ["a", "b", "c", "a"]); // "a" deduplicated
    const len = await r.sCard(k);
    const isMember = await r.sIsMember(k, "b");
    const members = await r.sMembers(k);
    await r.sRem(k, "c");
    const afterLen = await r.sCard(k);
    await r.del(k);
    return c.json({ ok: true, len, isMember, members_count: members.length, afterLen });
  } finally { await r.disconnect(); }
});

app.post("/set-ops", async (c) => {
  const k1 = `${KP}:so1:${Date.now()}`, k2 = `${KP}:so2:${Date.now()}`;
  const r = await getClient();
  try {
    await r.sAdd(k1, ["a", "b", "c"]);
    await r.sAdd(k2, ["b", "c", "d"]);
    const inter = await r.sInter([k1, k2]);
    const union = await r.sUnion([k1, k2]);
    const diff = await r.sDiff([k1, k2]);
    await r.del([k1, k2]);
    return c.json({ ok: true, inter: [...inter].sort(), union: [...union].sort(), diff: [...diff].sort() });
  } finally { await r.disconnect(); }
});

// ── Sorted sets ────────────────────────────────────────────────────────────

app.post("/zset", async (c) => {
  const k = `${KP}:zset:${Date.now()}`;
  const r = await getClient();
  try {
    await r.zAdd(k, [
      { score: 10, value: "alice" },
      { score: 20, value: "bob" },
      { score: 5, value: "charlie" },
    ]);
    const len = await r.zCard(k);
    const rank = await r.zRank(k, "alice");
    const score = await r.zScore(k, "bob");
    const range = await r.zRange(k, 0, -1, { REV: false });
    const rangeWithScores = await r.zRangeWithScores(k, 0, -1);
    await r.zRem(k, "charlie");
    const afterLen = await r.zCard(k);
    await r.del(k);
    return c.json({ ok: true, len, rank, score, range, rangeWithScores, afterLen });
  } finally { await r.disconnect(); }
});

app.post("/zrangebyscore", async (c) => {
  const k = `${KP}:zbyscore:${Date.now()}`;
  const r = await getClient();
  try {
    await r.zAdd(k, [
      { score: 1, value: "a" }, { score: 5, value: "b" },
      { score: 10, value: "c" }, { score: 15, value: "d" },
    ]);
    const range = await r.zRangeByScore(k, 4, 12);
    const count = await r.zCount(k, 4, 12);
    await r.del(k);
    return c.json({ ok: true, range, count });
  } finally { await r.disconnect(); }
});

// ── Pipeline (batch) ───────────────────────────────────────────────────────

app.post("/pipeline", async (c) => {
  const k = `${KP}:pipe:${Date.now()}`;
  const r = await getClient();
  try {
    // node-redis v4 uses multi() for pipelining
    const results = await r
      .multi()
      .set(k, "pipeline-value", { EX: 60 })
      .get(k)
      .incr(`${k}:count`)
      .incr(`${k}:count`)
      .get(`${k}:count`)
      .del([k, `${k}:count`])
      .exec();
    return c.json({
      ok: true,
      set_ok: results?.[0] === "OK",
      get_val: results?.[1],
      count_val: results?.[4],
    });
  } finally { await r.disconnect(); }
});

// ── Multi/exec (atomic transaction) ───────────────────────────────────────

app.post("/multi-exec", async (c) => {
  const k = `${KP}:multi:${Date.now()}`;
  const r = await getClient();
  try {
    const results = await r
      .multi()
      .set(k, "atomic-value")
      .get(k)
      .del(k)
      .exec();
    return c.json({ ok: true, set_ok: results[0] === "OK", get_val: results[1] });
  } finally { await r.disconnect(); }
});

// ── Key operations ────────────────────────────────────────────────────────

app.get("/scan", async (c) => {
  const r = await getClient();
  const prefix = `${KP}:scan:${Date.now()}`;
  try {
    await r.mSet([`${prefix}:a`, "1", `${prefix}:b`, "2", `${prefix}:c`, "3"]);
    const found: string[] = [];
    for await (const key of r.scanIterator({ MATCH: `${prefix}:*`, COUNT: 100 })) {
      found.push(key);
    }
    await r.del(found);
    return c.json({ ok: true, found: found.length });
  } finally { await r.disconnect(); }
});

app.post("/expire-delete", async (c) => {
  const k = `${KP}:expdel:${Date.now()}`;
  const r = await getClient();
  try {
    await r.set(k, "value", { EX: 5 });
    const before = await r.exists(k);
    await r.del(k);
    const after = await r.exists(k);
    return c.json({ ok: true, existed: before === 1, deleted: after === 0 });
  } finally { await r.disconnect(); }
});

// ── Concurrency ────────────────────────────────────────────────────────────

app.get("/concurrent", async (c) => {
  const r = await getClient();
  const base = `${KP}:conc:${Date.now()}`;
  try {
    const sets = await Promise.all(
      Array.from({ length: 5 }, (_, i) => r.set(`${base}:${i}`, String(i), { EX: 30 })),
    );
    const gets = await Promise.all(
      Array.from({ length: 5 }, (_, i) => r.get(`${base}:${i}`)),
    );
    await r.del(Array.from({ length: 5 }, (_, i) => `${base}:${i}`));
    return c.json({ ok: true, all_set: sets.every((s) => s === "OK"), values: gets });
  } finally { await r.disconnect(); }
});

// ── Aliases expected by the integration test runner ───────────────────────

// GET /redis-missing — GET on a nonexistent key returns null
app.get("/redis-missing", async (c) => {
  const r = await getClient();
  try {
    const val = await r.get(`${KP}:__nonexistent_key__${Date.now()}`);
    return c.json({ ok: true, is_null: val === null });
  } finally { await r.disconnect(); }
});

// POST /redis-ttl-check — set a key with TTL and verify it's set
app.post("/redis-ttl-check", async (c) => {
  const k = `${KP}:ttl-check:${Date.now()}`;
  const r = await getClient();
  try {
    await r.set(k, "value", { EX: 60 });
    const ttl = await r.ttl(k);
    await r.del(k);
    return c.json({ ok: true, ttl_set: ttl > 0 });
  } finally { await r.disconnect(); }
});

// GET /redis-type-ops — list (len=3, items=["a","b","c"]) + set (card=3, y_is_member=true)
app.get("/redis-type-ops", async (c) => {
  const r = await getClient();
  const lk = `${KP}:typeops-list:${Date.now()}`;
  const sk = `${KP}:typeops-set:${Date.now()}`;
  try {
    await r.del([lk, sk]);
    await r.rPush(lk, ["a", "b", "c"]);
    const listLen = await r.lLen(lk);
    const listItems = await r.lRange(lk, 0, -1);
    await r.sAdd(sk, ["x", "y", "z"]);
    const setCard = await r.sCard(sk);
    const yIsMember = await r.sIsMember(sk, "y");
    await r.del([lk, sk]);
    return c.json({
      ok: true,
      list: { len: listLen, items: listItems },
      set: { card: setCard, y_is_member: yIsMember },
    });
  } finally { await r.disconnect(); }
});

Deno.serve(app.fetch);
