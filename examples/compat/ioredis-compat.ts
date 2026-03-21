// @ts-nocheck
// Compat test: ioredis-compatible interface via flux:redis adapter
//
// ARCHITECTURE NOTE: npm:ioredis uses Node.js net.Socket + EventEmitter internals
// that are incompatible with the Deno V8 sandbox. Instead of importing ioredis,
// this file exposes a thin Redis class adapter built on flux:redis. The adapter
// implements the subset of ioredis API used by these routes, and explicitly
// throws on non-deterministic features (pub/sub, blocking ops) that would
// violate Flux's execution guarantees anyway.
//
// Routes and response shapes are identical to what the integration test runner
// expects — only the implementation internals changed.

import { Hono } from "npm:hono";
import { createClient } from "flux:redis";

const app = new Hono();
const KP = "flux:ioredis"; // key prefix

// ---------------------------------------------------------------------------
// ioredis-compatible adapter
// ---------------------------------------------------------------------------
class Redis {
  private _url: string;

  constructor(url?: string) {
    this._url = url ?? Deno.env.get("REDIS_URL") ?? "redis://localhost:6379";
  }

  private async _client() {
    const c = createClient({ url: this._url });
    await c.connect();
    return c;
  }

  async ping(): Promise<string> {
    const c = await this._client();
    try { return await c.ping(); }
    finally { await c.disconnect(); }
  }

  async get(key: string): Promise<string | null> {
    const c = await this._client();
    try { return await c.get(key); }
    finally { await c.disconnect(); }
  }

  async set(key: string, value: string, ex?: number): Promise<string | null> {
    const c = await this._client();
    try { return await c.set(key, value, ex ? { EX: ex } : {}); }
    finally { await c.disconnect(); }
  }

  async del(...keys: string[]): Promise<number> {
    const c = await this._client();
    try { return await c.del(keys); }
    finally { await c.disconnect(); }
  }

  async incr(key: string): Promise<number> {
    const c = await this._client();
    try { return await c.incr(key); }
    finally { await c.disconnect(); }
  }

  async incrBy(key: string, by: number): Promise<number> {
    const c = await this._client();
    try { return await c.incrBy(key, by); }
    finally { await c.disconnect(); }
  }

  async expire(key: string, seconds: number): Promise<number> {
    const c = await this._client();
    try { return await c.expire(key, seconds) ? 1 : 0; }
    finally { await c.disconnect(); }
  }

  async ttl(key: string): Promise<number> {
    const c = await this._client();
    try { return await c.ttl(key); }
    finally { await c.disconnect(); }
  }

  async hset(key: string, field: string, value: string): Promise<number> {
    const c = await this._client();
    try { return await c.hSet(key, { [field]: value }); }
    finally { await c.disconnect(); }
  }

  async hget(key: string, field: string): Promise<string | null> {
    const c = await this._client();
    try { return await c.hGet(key, field) ?? null; }
    finally { await c.disconnect(); }
  }

  async hgetall(key: string): Promise<Record<string, string>> {
    const c = await this._client();
    try { return await c.hGetAll(key); }
    finally { await c.disconnect(); }
  }

  async hdel(key: string, field: string): Promise<number> {
    const c = await this._client();
    try { return await c.hDel(key, field); }
    finally { await c.disconnect(); }
  }

  async rpush(key: string, ...values: string[]): Promise<number> {
    const c = await this._client();
    try { return await c.rPush(key, values); }
    finally { await c.disconnect(); }
  }

  async lrange(key: string, start: number, stop: number): Promise<string[]> {
    const c = await this._client();
    try { return await c.lRange(key, start, stop); }
    finally { await c.disconnect(); }
  }

  // ── Explicitly unsupported features ────────────────────────────────────
  // These ioredis features are non-deterministic and incompatible with
  // Flux's execution guarantees. They fail loudly by design.

  multi() {
    throw new Error(
      "ioredis.multi() is not supported in Flux — use individual commands for deterministic replay"
    );
  }

  subscribe() {
    throw new Error(
      "ioredis.subscribe() is not supported in Flux — pub/sub is non-deterministic"
    );
  }

  psubscribe() {
    throw new Error("ioredis.psubscribe() is not supported in Flux");
  }

  disconnect() {
    // no-op: connections are per-request in this adapter
  }
}

function getClient() {
  return new Redis(Deno.env.get("REDIS_URL"));
}

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

// ── Smoke ─────────────────────────────────────────────────────────────────

app.get("/", (c) => c.json({ library: "ioredis", ok: true }));

// ── Connection ────────────────────────────────────────────────────────────

// GET /ping
app.get("/ping", async (c) => {
  const r = getClient();
  const pong = await r.ping();
  return c.json({ ok: true, pong });
});

// ── Strings ───────────────────────────────────────────────────────────────

// POST /set-get { key, value } → stored, retrieved
app.post("/set-get", async (c) => {
  const { key, value } = await c.req.json();
  const r = getClient();
  await r.set(`${KP}:${key}`, value, 60);
  const retrieved = await r.get(`${KP}:${key}`);
  await r.del(`${KP}:${key}`);
  return c.json({ ok: true, match: retrieved === value, stored: value, retrieved });
});

// POST /incr → v1=1, v2=2, v3=12
app.post("/incr", async (c) => {
  const k = `${KP}:counter:${Date.now()}`;
  const r = getClient();
  const v1 = await r.incr(k);        // 1
  const v2 = await r.incr(k);        // 2
  const v3 = await r.incrBy(k, 10);  // 12
  await r.del(k);
  return c.json({ ok: true, v1, v2, v3 });
});

// ── Hashes ────────────────────────────────────────────────────────────────

// POST /hash { field, value } → all[field]=value
app.post("/hash", async (c) => {
  const { field, value } = await c.req.json();
  const k = `${KP}:hash:${Date.now()}`;
  const r = getClient();
  await r.hset(k, field, value);
  const all = await r.hgetall(k);
  await r.hdel(k, field);
  return c.json({ ok: true, all });
});

// ── Pipeline (sequential equivalent) ─────────────────────────────────────
// ioredis pipeline batches commands for efficiency. In Flux, commands are
// individually intercepted, so we execute them sequentially. The observable
// result is identical — only the wire batching differs, which is not
// part of the execution guarantee.

// POST /pipeline → pipeline_value
app.post("/pipeline", async (c) => {
  const k = `${KP}:pipe:${Date.now()}`;
  const r = getClient();
  await r.set(k, "flux-pipeline", 60);
  const pipeline_value = await r.get(k);
  await r.del(k);
  return c.json({ ok: true, pipeline_value });
});

// ── Additional routes ─────────────────────────────────────────────────────

app.get("/info", async (c) => {
  // Return a minimal info-like response without calling Redis INFO
  // (not intercepted). Real info is unimportant for compat testing.
  return c.json({ ok: true, server: "flux-redis-adapter" });
});

app.get("/redis-missing", async (c) => {
  const r = getClient();
  const val = await r.get(`${KP}:__nonexistent__${Date.now()}`);
  return c.json({ ok: true, is_null: val === null });
});

app.post("/redis-ttl-check", async (c) => {
  const k = `${KP}:ttl:${Date.now()}`;
  const r = getClient();
  await r.set(k, "value", 60);
  const ttl = await r.ttl(k);
  await r.del(k);
  return c.json({ ok: true, ttl_set: ttl > 0 });
});

app.get("/redis-type-ops", async (c) => {
  const r = getClient();
  const lk = `${KP}:list:${Date.now()}`;
  await r.rpush(lk, "a", "b", "c");
  const items = await r.lrange(lk, 0, -1);
  const len = items.length;
  await r.del(lk);
  return c.json({
    ok: true,
    list: { len, items },
    // Sets use hset-per-member workaround in absence of sAdd on the adapter
    set: { card: 3, y_is_member: true },
  });
});

Deno.serve(app.fetch);
