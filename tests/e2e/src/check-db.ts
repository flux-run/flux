import pg from "flux:pg";

const pool = new pg.Pool({
  connectionString: Deno.env.get("DATABASE_URL"),
});

try {
  const res = await pool.query("SELECT COUNT(*) as count FROM orders");
  console.log(`📊 Total orders in DB: ${res.rows[0].count}`);
} catch (err) {
  console.error("❌ Failed to query DB:", err);
  Deno.exit(1);
} finally {
  await pool.end();
}
