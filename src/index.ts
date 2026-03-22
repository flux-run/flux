// @ts-nocheck
import { Hono } from "https://esm.sh/hono@3.11.7";
import { z } from "https://esm.sh/zod@3.22.4";
import { drizzle } from "https://esm.sh/drizzle-orm@0.31.0/pg-proxy";
import { pgTable, serial, text, integer } from "https://esm.sh/drizzle-orm@0.31.0/pg-core";
import pg from "flux:pg";

const app = new Hono();

// 1. Drizzle Schema
const orders = pgTable("flux_demo_orders", {
  id: serial("id").primaryKey(),
  email: text("email").notNull(),
  amount: integer("amount").notNull(),
});

// 2. Database Setup
const DB_URL = Deno.env.get("DATABASE_URL") || "postgres://ep-red-water-a1cnxz0z-pooler.ap-southeast-1.aws.neon.tech/neondb";
console.log("Flux App DB_URL:", DB_URL);
const pool = new pg.Pool({ connectionString: DB_URL });

const db = drizzle(async (sql, params, method) => {
  try {
    const result = await pool.query(sql, params);
    return { rows: result.rows.map(row => Object.values(row)) };
  } catch (err) {
    console.error("Drizzle Proxy Error:", err);
    throw err;
  }
});

// 3. App Handlers
app.get("/", (c) => c.json({ status: "ok", service: "flux-demo" }));

app.get("/orders", async (c) => {
  const allOrders = await db.select().from(orders);
  return c.json(allOrders);
});

app.post("/orders", async (c) => {
  const body = await c.req.json();
  const schema = z.object({
    email: z.string().email(),
    amount: z.number().int().positive().optional(),
    productId: z.string().optional(),
  });
  const parsed = schema.parse(body);
  const email = parsed.email;
  const amount = parsed.amount || parseInt(parsed.productId || "0");

  // Ensure table exists (best effort)
  await pool.query(`
    CREATE TABLE IF NOT EXISTS flux_demo_orders (
      id SERIAL PRIMARY KEY,
      email TEXT NOT NULL,
      amount INTEGER NOT NULL
    )
  `);

  const inserted = await db.insert(orders).values({ email, amount }).returning();
  return c.json({ ...inserted[0], status: "completed" });
});

app.get("/fail", (c) => {
  throw new Error("Simulated failure for Flux Demo");
});

Deno.serve(app.fetch);
