import type { CreateTodoInput, Todo, UpdateTodoInput } from "./schema_flux.ts";

export interface TodoRepository {
  list(): Promise<Todo[]>;
  findById(id: number): Promise<Todo | null>;
  create(input: CreateTodoInput): Promise<Todo>;
  update(id: number, input: UpdateTodoInput): Promise<Todo | null>;
  remove(id: number): Promise<boolean>;
}