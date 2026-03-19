// @ts-nocheck
// Compat test: Drizzle ORM over pg via Flux.postgres
import { Hono } from "npm:hono";
import pg from "flux:pg";
import { drizzle } from "npm:drizzle-orm/node-postgres";
import { pgTable, serial, text, timestamp } from "npm:drizzle-orm/pg-core";
import { eq } from "npm:drizzle-orm";

// Schema definition (in-file for compat test simplicity)
const items = pgTable("flux_drizzle_compat_items", {
  id: serial("id").primaryKey(),
  name: text("name").notNull(),
  createdAt: timestamp("created_at").defaultNow(),
});

const app = new Hono();

function getDb() {
  const pool = new pg.Pool({ connectionString: Deno.env.get("DATABASE_URL") });
  return drizzle(pool);
}

// GET / — smoke test
app.get("/", (c) => c.json({ library: "drizzle", ok: true }));

// POST /setup — create the test table
app.post("/setup", async (c) => {
  const db = getDb();
  await db.execute(`
    CREATE TABLE IF NOT EXISTS flux_drizzle_compat_items (
      id SERIAL PRIMARY KEY,
      name TEXT NOT NULL,
      created_at TIMESTAMPTZ DEFAULT NOW()
    )
  `);
  return c.json({ ok: true });
});

// POST /insert — insert a row via Drizzle
app.post("/insert", async (c) => {
  const { name } = await c.req.json();
  const db = getDb();
  const result = await db.insert(items).values({ name }).returning();
  return c.json({ ok: true, item: result[0] });
});

// GET /list — select all rows via Drizzle
app.get("/list", async (c) => {
  const db = getDb();
  const rows = await db.select().from(items);
  return c.json({ ok: true, count: rows.length, items: rows });
});

// DELETE /cleanup — drop test table
app.delete("/cleanup", async (c) => {
  const db = getDb();
  await db.execute("DROP TABLE IF EXISTS flux_drizzle_compat_items");
  return c.json({ ok: true });
});

Deno.serve(app.fetch);
