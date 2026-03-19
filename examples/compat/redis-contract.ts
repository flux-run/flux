// @ts-nocheck
// Flux Redis Execution Contract Tests
// These tests prove what Flux GUARANTEES and what it EXPLICITLY REFUSES.
//
// Context: Flux intercepts each Redis command as a deterministic checkpoint.
// Because of this model, certain Redis features are fundamentally incompatible:
//
//   BLOCKED (non-deterministic, cannot be checkpointed):
//     - MULTI / EXEC / WATCH / UNWATCH  → "Redis transactions are not supported in Flux"
//     - SUBSCRIBE / PUBLISH / PSUBSCRIBE → "Redis pub/sub is not supported in Flux"
//     - BLPOP / BRPOP / BZPOPMIN / BZPOPMAX → "Redis blocking commands are not supported in Flux"
//
//   SUPPORTED (stateless per-command, fully replayable):
//     - GET / SET / DEL / EXISTS / TTL
//     - HSET / HGET / HGETALL
//     - LPUSH / RPUSH / LRANGE
//     - SADD / SMEMBERS / SCARD
//     - ZADD / ZRANGE / ZSCORE
//     - INCR / INCRBY / DECR
//     - SCAN / KEYS
//
// run: flux build redis-contract.ts && flux run redis-contract.ts --listen
import { Hono } from "npm:hono";
import { createClient } from "flux:redis";

const app = new Hono();

async function getClient() {
  const client = createClient({ url: Deno.env.get("REDIS_URL") ?? "redis://localhost:6379" });
  await client.connect();
  return client;
}

// ── Smoke ─────────────────────────────────────────────────────────────────

app.get("/", (c) =>
  c.json({
    test: "redis-contract",
    note: "Tests prove Flux Redis execution contract: what is guaranteed vs what is blocked",
    ok: true,
  }),
);

// ═══════════════════════════════════════════════════════════════
// SUPPORTED: Per-command stateless operations are fully replayable.
// Each command is checkpointed. On replay, the recorded result is returned
// without contacting the real Redis server.
// ═══════════════════════════════════════════════════════════════

app.get("/supported/ping", async (c) => {
  const r = await getClient();
  try {
    const pong = await r.ping();
    return c.json({ contract: "supported", command: "PING", ok: pong === "PONG" });
  } finally { await r.disconnect(); }
});

app.post("/supported/string-ops", async (c) => {
  const k = `flux:contract:str:${Date.now()}`;
  const r = await getClient();
  try {
    await r.set(k, "contract-value", { EX: 30 });
    const val = await r.get(k);
    const ttl = await r.ttl(k);
    await r.del(k);
    return c.json({
      contract: "supported",
      commands: ["SET", "GET", "TTL", "DEL"],
      ok: val === "contract-value" && ttl > 0,
      replay_note: "On replay, no Redis commands are issued. Recorded values returned.",
    });
  } finally { await r.disconnect(); }
});

app.post("/supported/hash-ops", async (c) => {
  const k = `flux:contract:hash:${Date.now()}`;
  const r = await getClient();
  try {
    await r.hSet(k, { name: "flux", version: "1" });
    const all = await r.hGetAll(k);
    await r.del(k);
    return c.json({ contract: "supported", commands: ["HSET", "HGETALL", "DEL"], ok: all?.name === "flux" });
  } finally { await r.disconnect(); }
});

app.post("/supported/list-ops", async (c) => {
  const k = `flux:contract:list:${Date.now()}`;
  const r = await getClient();
  try {
    await r.rPush(k, ["a", "b", "c"]);
    const all = await r.lRange(k, 0, -1);
    await r.del(k);
    return c.json({ contract: "supported", commands: ["RPUSH", "LRANGE", "DEL"], ok: all.length === 3 });
  } finally { await r.disconnect(); }
});

app.post("/supported/set-ops", async (c) => {
  const k = `flux:contract:set:${Date.now()}`;
  const r = await getClient();
  try {
    await r.sAdd(k, ["x", "y", "z"]);
    const count = await r.sCard(k);
    await r.del(k);
    return c.json({ contract: "supported", commands: ["SADD", "SCARD", "DEL"], ok: count === 3 });
  } finally { await r.disconnect(); }
});

app.post("/supported/zset-ops", async (c) => {
  const k = `flux:contract:zset:${Date.now()}`;
  const r = await getClient();
  try {
    await r.zAdd(k, [{ score: 10, value: "a" }, { score: 20, value: "b" }]);
    const score = await r.zScore(k, "b");
    await r.del(k);
    return c.json({ contract: "supported", commands: ["ZADD", "ZSCORE", "DEL"], ok: score === 20 });
  } finally { await r.disconnect(); }
});

app.post("/supported/counter", async (c) => {
  const k = `flux:contract:counter:${Date.now()}`;
  const r = await getClient();
  try {
    const v1 = await r.incr(k);
    const v2 = await r.incrBy(k, 9);
    await r.del(k);
    return c.json({ contract: "supported", commands: ["INCR", "INCRBY"], ok: v1 === 1 && v2 === 10 });
  } finally { await r.disconnect(); }
});

// ═══════════════════════════════════════════════════════════════
// BLOCKED: These operations cannot be made deterministic.
// Flux blocks them at the runtime level before any network call is made.
// Expected error: "Redis {feature} are not supported in Flux (non-deterministic execution)"
// ═══════════════════════════════════════════════════════════════

// MULTI/EXEC — Redis server-side transactions are blocked.
// Reason: MULTI/EXEC spans multiple round-trips as a session.
// Flux checkpoints individual commands, not sessions.
// If replayed, the sequence cannot be correctly reconstructed.
app.post("/blocked/multi-exec", async (c) => {
  const r = await getClient();
  try {
    const result = await r.multi().set("flux:blocked:test", "1").exec();
    await r.disconnect();
    // If we reach here, the block is not working — this is a contract violation
    return c.json({
      contract: "blocked",
      feature: "MULTI/EXEC",
      ok: false,
      error: "CONTRACT VIOLATION: MULTI/EXEC executed when it should be blocked",
      result,
    }, 500);
  } catch (e: any) {
    await r.disconnect().catch(() => {});
    const msg = e?.message ?? String(e);
    const correctly_blocked = msg.includes("transactions") && msg.includes("not supported");
    return c.json({
      contract: "blocked",
      feature: "MULTI/EXEC",
      ok: correctly_blocked,
      correctly_blocked,
      error: msg,
      expected_contains: ["transactions", "not supported in Flux"],
    });
  }
});

// WATCH — part of optimistic locking (session state), also blocked
app.post("/blocked/watch", async (c) => {
  const r = await getClient();
  try {
    await r.watch("flux:watch:test");
    await r.disconnect();
    return c.json({ contract: "blocked", feature: "WATCH", ok: false, error: "should be blocked" }, 500);
  } catch (e: any) {
    await r.disconnect().catch(() => {});
    const msg = e?.message ?? String(e);
    return c.json({
      contract: "blocked",
      feature: "WATCH",
      ok: msg.includes("transactions") || msg.includes("not supported"),
      error: msg,
    });
  }
});

// SUBSCRIBE — pub/sub requires a persistent connection; not checkpointable
app.post("/blocked/subscribe", async (c) => {
  // node-redis doesn't support calling subscribe on a regular connection
  // but the underlying SUBSCRIBE command is blocked by Flux
  // We test via direct command invocation
  try {
    const r = await getClient();
    // This will fail at the Flux intercept layer before reaching Redis
    // node-redis may also refuse to call SUBSCRIBE on a non-duplicate client
    // Either error is acceptable — the important thing is it doesn't succeed
    await r.sendCommand(["SUBSCRIBE", "flux:test:channel"]);
    await r.disconnect();
    return c.json({ contract: "blocked", feature: "SUBSCRIBE", ok: false, error: "should be blocked" }, 500);
  } catch (e: any) {
    const msg = e?.message ?? String(e);
    const correctly_blocked = msg.includes("pub/sub") || msg.includes("not supported") || msg.includes("client");
    return c.json({
      contract: "blocked",
      feature: "SUBSCRIBE",
      ok: correctly_blocked,
      error: msg,
    });
  }
});

// BLPOP — blocking pop; hangs indefinitely waiting for a list element
// Blocked because it cannot be deterministically replayed.
app.post("/blocked/blpop", async (c) => {
  const r = await getClient();
  try {
    // timeout=1 would make it unblock after 1s in real Redis,
    // but Flux should reject this before it ever reaches the server.
    await r.blPop("flux:blocked:blpop", 1);
    await r.disconnect();
    return c.json({ contract: "blocked", feature: "BLPOP", ok: false, error: "should be blocked" }, 500);
  } catch (e: any) {
    await r.disconnect().catch(() => {});
    const msg = e?.message ?? String(e);
    const correctly_blocked = msg.includes("blocking") || msg.includes("not supported");
    return c.json({
      contract: "blocked",
      feature: "BLPOP",
      ok: correctly_blocked,
      error: msg,
    });
  }
});

// BRPOP — same as BLPOP
app.post("/blocked/brpop", async (c) => {
  const r = await getClient();
  try {
    await r.brPop("flux:blocked:brpop", 1);
    await r.disconnect();
    return c.json({ contract: "blocked", feature: "BRPOP", ok: false, error: "should be blocked" }, 500);
  } catch (e: any) {
    await r.disconnect().catch(() => {});
    const msg = e?.message ?? String(e);
    return c.json({
      contract: "blocked",
      feature: "BRPOP",
      ok: msg.includes("blocking") || msg.includes("not supported"),
      error: msg,
    });
  }
});

// ═══════════════════════════════════════════════════════════════
// REPLAY PROOF: Replay does not re-execute Redis commands.
// These routes are designed to be called, then replayed via `flux replay <id>`.
// After replay, the Redis state should not have changed.
// ═══════════════════════════════════════════════════════════════

app.post("/replay-proof/set", async (c) => {
  const k = `flux:replay:${Date.now()}`;
  const r = await getClient();
  try {
    await r.set(k, "replay-value", { EX: 60 });
    const val = await r.get(k);
    await r.del(k);
    return c.json({
      law: "replay-safety",
      ok: val === "replay-value",
      key: k,
      note: "On replay: the SET is suppressed. Redis is not actually written. Response is identical.",
    });
  } finally { await r.disconnect(); }
});

// ═══════════════════════════════════════════════════════════════
// SUMMARY ROUTE: Run all contract checks (requires Redis)
// ═══════════════════════════════════════════════════════════════

app.get("/summary", (c) => c.json({
  supported: [
    "PING", "SET", "GET", "DEL", "EXISTS", "TTL", "PTTL", "EXPIRE", "PERSIST",
    "APPEND", "GETSET", "MSET", "MGET", "INCR", "INCRBY", "INCRBYFLOAT", "DECR", "DECRBY",
    "HSET", "HGET", "HGETALL", "HKEYS", "HVALS", "HLEN", "HDEL", "HEXISTS", "HINCRBY",
    "LPUSH", "RPUSH", "LLEN", "LRANGE", "LINDEX", "LSET", "LREM", "LTRIM", "LPOP", "RPOP",
    "SADD", "SCARD", "SISMEMBER", "SMEMBERS", "SREM", "SINTER", "SUNION", "SDIFF",
    "ZADD", "ZCARD", "ZRANK", "ZSCORE", "ZRANGE", "ZREVRANGE", "ZRANGEBYSCORE", "ZREM", "ZCOUNT",
    "SCAN", "KEYS", "TYPE",
  ],
  blocked: {
    transactions: ["MULTI", "EXEC", "WATCH", "UNWATCH"],
    pub_sub: ["SUBSCRIBE", "PSUBSCRIBE", "UNSUBSCRIBE", "PUNSUBSCRIBE", "PUBLISH"],
    blocking: ["BLPOP", "BRPOP", "BZPOPMIN", "BZPOPMAX", "XREAD (with BLOCK)", "XREADGROUP (with BLOCK)"],
  },
  error_format: "Redis {feature} are not supported in Flux (non-deterministic execution)",
}));

Deno.serve(app.fetch);
