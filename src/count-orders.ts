// @ts-nocheck
import pg from "flux:pg";

const DB_URL = Deno.env.get("DATABASE_URL");
const pool = new pg.Pool({ connectionString: DB_URL });

export default async function handler() {
  await pool.query(`
    CREATE TABLE IF NOT EXISTS flux_demo_orders (
      id SERIAL PRIMARY KEY,
      email TEXT NOT NULL,
      amount INTEGER NOT NULL
    )
  `);
  const result = await pool.query("SELECT COUNT(*) as count FROM flux_demo_orders");
  console.log("COUNT:", result.rows[0].count);
  return { count: result.rows[0].count };
}
