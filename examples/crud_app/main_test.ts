import { assertEquals } from "@std/assert";
import { createApp } from "./src/app.ts";
import { createInMemoryTodoRepository } from "./src/repository.ts";

Deno.test("creates and lists todos", async () => {
  const app = createApp(createInMemoryTodoRepository());

  const createResponse = await app.request("/todos", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      title: "Ship CRUD example",
      description: "Use Hono, Drizzle, Zod, and Postgres",
    }),
  });

  assertEquals(createResponse.status, 201);

  const createdTodo = await createResponse.json();
  assertEquals(createdTodo.title, "Ship CRUD example");
  assertEquals(createdTodo.completed, false);

  const listResponse = await app.request("/todos");
  assertEquals(listResponse.status, 200);

  const todos = await listResponse.json();
  assertEquals(todos.length, 1);
  assertEquals(todos[0].id, createdTodo.id);
});

Deno.test("updates and deletes todos", async () => {
  const app = createApp(createInMemoryTodoRepository());

  const createResponse = await app.request("/todos", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ title: "Initial title" }),
  });

  const createdTodo = await createResponse.json();

  const updateResponse = await app.request(`/todos/${createdTodo.id}`, {
    method: "PUT",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ completed: true, title: "Updated title" }),
  });

  assertEquals(updateResponse.status, 200);

  const updatedTodo = await updateResponse.json();
  assertEquals(updatedTodo.title, "Updated title");
  assertEquals(updatedTodo.completed, true);

  const deleteResponse = await app.request(`/todos/${createdTodo.id}`, {
    method: "DELETE",
  });

  assertEquals(deleteResponse.status, 204);

  const getResponse = await app.request(`/todos/${createdTodo.id}`);
  assertEquals(getResponse.status, 404);
});

Deno.test("rejects invalid payloads", async () => {
  const app = createApp(createInMemoryTodoRepository());

  const response = await app.request("/todos", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ title: "" }),
  });

  assertEquals(response.status, 422);

  const body = await response.json();
  assertEquals(body.error, "Validation failed");
});
