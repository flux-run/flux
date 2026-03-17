import { Hono } from "npm:hono";
import { z, ZodError } from "npm:zod";

import type { TodoRepository } from "./repository_flux.ts";
import { createTodoSchema, todoIdParamSchema, updateTodoSchema } from "./schema_flux.ts";

function validationError(error: ZodError) {
  return {
    error: "Validation failed",
    issues: error.issues.map((issue) => ({
      path: issue.path.join("."),
      message: issue.message,
    })),
  };
}

async function parseJson<T>(request: Request, schema: z.ZodType<T>) {
  const payload = await request.json();
  return schema.parse(payload);
}

export function createApp(repository: TodoRepository) {
  const app = new Hono();

  app.get("/", (c) => {
    return c.json({
      name: "crud_app",
      endpoints: ["GET /todos", "GET /todos/:id", "POST /todos", "PUT /todos/:id", "DELETE /todos/:id"],
    });
  });

  app.get("/todos", async (c) => {
    const todos = await repository.list();
    return c.json(todos);
  });

  app.get("/todos/:id", async (c) => {
    try {
      const { id } = todoIdParamSchema.parse(c.req.param());
      const todo = await repository.findById(id);

      if (!todo) {
        return c.json({ error: "Todo not found" }, 404);
      }

      return c.json(todo);
    } catch (error) {
      if (error instanceof ZodError) {
        return c.json(validationError(error), 422);
      }

      throw error;
    }
  });

  app.post("/todos", async (c) => {
    try {
      const input = await parseJson(c.req.raw, createTodoSchema);
      const todo = await repository.create(input);
      return c.json(todo, 201);
    } catch (error) {
      if (error instanceof ZodError) {
        return c.json(validationError(error), 422);
      }

      if (error instanceof SyntaxError) {
        return c.json({ error: "Request body must be valid JSON" }, 400);
      }

      throw error;
    }
  });

  app.put("/todos/:id", async (c) => {
    try {
      const { id } = todoIdParamSchema.parse(c.req.param());
      const input = await parseJson(c.req.raw, updateTodoSchema);
      const todo = await repository.update(id, input);

      if (!todo) {
        return c.json({ error: "Todo not found" }, 404);
      }

      return c.json(todo);
    } catch (error) {
      if (error instanceof ZodError) {
        return c.json(validationError(error), 422);
      }

      if (error instanceof SyntaxError) {
        return c.json({ error: "Request body must be valid JSON" }, 400);
      }

      throw error;
    }
  });

  app.delete("/todos/:id", async (c) => {
    try {
      const { id } = todoIdParamSchema.parse(c.req.param());
      const deleted = await repository.remove(id);

      if (!deleted) {
        return c.json({ error: "Todo not found" }, 404);
      }

      return c.body(null, 204);
    } catch (error) {
      if (error instanceof ZodError) {
        return c.json(validationError(error), 422);
      }

      throw error;
    }
  });

  return app;
}