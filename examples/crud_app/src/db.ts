import { and, desc, eq } from "npm:drizzle-orm";
import { drizzle } from "npm:drizzle-orm/postgres-js";
import postgres from "npm:postgres";

import type { TodoRepository } from "./repository.ts";
import { todos, type CreateTodoInput, type Todo, type UpdateTodoInput } from "./schema.ts";

type PostgresTodoRepository = TodoRepository & {
  sql: ReturnType<typeof postgres>;
};

export function createPostgresTodoRepository(): PostgresTodoRepository {
  const databaseUrl = Deno.env.get("DATABASE_URL");

  if (!databaseUrl) {
    throw new Error("DATABASE_URL is required to run the CRUD app.");
  }

  const sqlClient = postgres(databaseUrl, { prepare: false });
  const db = drizzle(sqlClient);

  return {
    sql: sqlClient,

    async list() {
      return db.select().from(todos).orderBy(desc(todos.id));
    },

    async findById(id) {
      const [todo] = await db.select().from(todos).where(eq(todos.id, id)).limit(1);
      return todo ?? null;
    },

    async create(input: CreateTodoInput) {
      const [todo] = await db.insert(todos).values({
        title: input.title,
        description: input.description,
        completed: input.completed ?? false,
      }).returning();

      return todo as Todo;
    },

    async update(id: number, input: UpdateTodoInput) {
      const payload = {
        ...input,
        updatedAt: new Date(),
      };

      const [todo] = await db.update(todos)
        .set(payload)
        .where(and(eq(todos.id, id)))
        .returning();

      return todo ?? null;
    },

    async remove(id: number) {
      const deleted = await db.delete(todos).where(eq(todos.id, id)).returning({ id: todos.id });
      return deleted.length > 0;
    },
  };
}

export async function ensureSchema(sqlClient: ReturnType<typeof postgres>) {
  await sqlClient.unsafe(`
    CREATE TABLE IF NOT EXISTS todos (
      id integer PRIMARY KEY GENERATED ALWAYS AS IDENTITY,
      title varchar(255) NOT NULL,
      description text,
      completed boolean NOT NULL DEFAULT false,
      created_at timestamptz NOT NULL DEFAULT now(),
      updated_at timestamptz NOT NULL DEFAULT now()
    )
  `);

  await sqlClient.unsafe(`
    CREATE OR REPLACE FUNCTION set_todos_updated_at()
    RETURNS TRIGGER AS $$
    BEGIN
      NEW.updated_at = now();
      RETURN NEW;
    END;
    $$ LANGUAGE plpgsql
  `);

  await sqlClient.unsafe(`
    DROP TRIGGER IF EXISTS todos_set_updated_at ON todos;
    CREATE TRIGGER todos_set_updated_at
    BEFORE UPDATE ON todos
    FOR EACH ROW
    EXECUTE FUNCTION set_todos_updated_at();
  `);
}