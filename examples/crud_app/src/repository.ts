import type { CreateTodoInput, Todo, UpdateTodoInput } from "./schema.ts";

export interface TodoRepository {
  list(): Promise<Todo[]>;
  findById(id: number): Promise<Todo | null>;
  create(input: CreateTodoInput): Promise<Todo>;
  update(id: number, input: UpdateTodoInput): Promise<Todo | null>;
  remove(id: number): Promise<boolean>;
}

export function createInMemoryTodoRepository(): TodoRepository {
  const store = new Map<number, Todo>();
  let currentId = 1;

  return {
    async list() {
      return [...store.values()].sort((left, right) => left.id - right.id);
    },

    async findById(id) {
      return store.get(id) ?? null;
    },

    async create(input) {
      const now = new Date();
      const todo: Todo = {
        id: currentId++,
        title: input.title,
        description: input.description ?? null,
        completed: input.completed ?? false,
        createdAt: now,
        updatedAt: now,
      };

      store.set(todo.id, todo);
      return todo;
    },

    async update(id, input) {
      const existing = store.get(id);

      if (!existing) {
        return null;
      }

      const updated: Todo = {
        ...existing,
        ...input,
        description: input.description ?? existing.description,
        updatedAt: new Date(),
      };

      store.set(id, updated);
      return updated;
    },

    async remove(id) {
      return store.delete(id);
    },
  };
}