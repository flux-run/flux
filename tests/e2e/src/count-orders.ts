import pg from "flux:pg";

const pool = new pg.Pool({
  connectionString: Deno.env.get("DATABASE_URL"),
});

async function main() {
  const result = await pool.query("SELECT COUNT(*) as count FROM orders");
  console.log(`COUNT: ${result.rows[0].count}`);
  await pool.end();
}

main();
