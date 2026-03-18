import { boolean, integer, pgTable, text, timestamp, varchar } from "npm:drizzle-orm/pg-core";
import { createInsertSchema, createSelectSchema } from "npm:drizzle-zod";
import { z } from "npm:zod";

export const todos = pgTable("todos", {
  id: integer("id").primaryKey().generatedAlwaysAsIdentity(),
  title: varchar("title", { length: 255 }).notNull(),
  description: text("description"),
  completed: boolean("completed").notNull().default(false),
  createdAt: timestamp("created_at", { withTimezone: true }).notNull().defaultNow(),
  updatedAt: timestamp("updated_at", { withTimezone: true }).notNull().defaultNow(),
});

export const todoSelectSchema = createSelectSchema(todos);

export const todoInsertSchema = createInsertSchema(todos);

export const createTodoSchema = z.object({
  title: z.string().min(1).max(255),
  description: z.string().max(1000).optional(),
  completed: z.boolean().optional(),
});

export const updateTodoSchema = createTodoSchema.partial().refine(
  (value) => Object.keys(value).length > 0,
  { message: "At least one field must be provided." },
);

export const todoIdParamSchema = z.object({
  id: z.coerce.number().int().positive(),
});

export type Todo = z.infer<typeof todoSelectSchema>;
export type CreateTodoInput = z.infer<typeof createTodoSchema>;
export type UpdateTodoInput = z.infer<typeof updateTodoSchema>;