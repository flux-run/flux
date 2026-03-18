import pg from "../../flux-pg.js";

import type { TodoRepository } from "./repository_flux.ts";
import { todoSchema, type CreateTodoInput, type Todo, type UpdateTodoInput } from "./schema_flux.ts";

type FluxPool = {
  query: (query: string | { text: string; values?: unknown[]; rowMode?: "array" }, params?: unknown[]) => Promise<{ rows: Record<string, unknown>[]; rowCount?: number }>;
  end: () => Promise<void>;
};

type FluxTodoRepository = TodoRepository & {
  pool: FluxPool;
};

export function createFluxTodoRepository(): FluxTodoRepository {
  const databaseUrl = Deno.env.get("DATABASE_URL");

  if (!databaseUrl) {
    throw new Error("DATABASE_URL is required to run the CRUD app on Flux.");
  }

  const pool = new pg.Pool({
    connectionString: databaseUrl,
  }) as FluxPool;

  return {
    pool,

    async list() {
      const result = await pool.query(`
        SELECT id, title, description, completed, created_at, updated_at
        FROM todos
        ORDER BY id DESC
      `);

      return result.rows.map(mapTodoRow);
    },

    async findById(id) {
      const result = await pool.query(
        `
          SELECT id, title, description, completed, created_at, updated_at
          FROM todos
          WHERE id = $1
          LIMIT 1
        `,
        [id],
      );
      const [todo] = result.rows;
      return todo ?? null;
    },

    async create(input: CreateTodoInput) {
      const result = await pool.query(
        `
          INSERT INTO todos (title, description, completed)
          VALUES ($1, $2, $3)
          RETURNING id, title, description, completed, created_at, updated_at
        `,
        [input.title, input.description ?? null, input.completed ?? false],
      );

      return mapTodoRow(result.rows[0]);
    },

    async update(id: number, input: UpdateTodoInput) {
      const result = await pool.query(
        `
          UPDATE todos
          SET
            title = COALESCE($2, title),
            description = COALESCE($3, description),
            completed = COALESCE($4, completed),
            updated_at = $5
          WHERE id = $1
          RETURNING id, title, description, completed, created_at, updated_at
        `,
        [
          id,
          input.title ?? null,
          input.description ?? null,
          input.completed ?? null,
          new Date(),
        ],
      );

      const [todo] = result.rows;
      return todo ? mapTodoRow(todo) : null;
    },

    async remove(id: number) {
      const result = await pool.query(
        "DELETE FROM todos WHERE id = $1",
        [id],
      );
      return (result.rowCount ?? 0) > 0;
    },
  };
}

function mapTodoRow(row: Record<string, unknown>): Todo {
  return todoSchema.parse({
    id: row.id,
    title: row.title,
    description: row.description ?? null,
    completed: row.completed,
    createdAt: row.created_at,
    updatedAt: row.updated_at,
  });
}