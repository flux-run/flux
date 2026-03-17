import { eq, sql } from "drizzle-orm";
import { integer, pgTable, text } from "drizzle-orm/pg-core";
import { drizzle } from "drizzle-orm/node-postgres";
import { Pool } from "pg";

type Input = {
  connectionString: string;
};

const todos = pgTable("flux_drizzle_todos", {
  id: integer("id").primaryKey().generatedAlwaysAsIdentity(),
  title: text("title").notNull(),
  state: text("state").notNull(),
});

export default async function handler({ input }: { input: Input }) {
  const pool = new Pool({ connectionString: input.connectionString });
  const db = drizzle(pool);

  try {
    await db.execute(sql`drop table if exists flux_drizzle_todos`);
    await db.execute(sql`
      create table flux_drizzle_todos (
        id integer generated always as identity primary key,
        title text not null,
        state text not null
      )
    `);

    const inserted = await db
      .insert(todos)
      .values({ title: "ship flux", state: "new" })
      .returning();

    const selected = await db
      .select()
      .from(todos)
      .where(eq(todos.id, inserted[0].id));

    const updated = await db
      .update(todos)
      .set({ state: "done" })
      .where(eq(todos.id, inserted[0].id))
      .returning();

    return {
      inserted: inserted[0],
      selected: selected[0],
      updated: updated[0],
    };
  } finally {
    try {
      await db.execute(sql`drop table if exists flux_drizzle_todos`);
    } catch {
      // Best-effort cleanup so reruns stay simple.
    }
    await pool.end();
  }
}