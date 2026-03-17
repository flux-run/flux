import { z } from "npm:zod";

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

export const todoSchema = z.object({
  id: z.number().int().positive(),
  title: z.string(),
  description: z.string().nullable(),
  completed: z.boolean(),
  createdAt: z.coerce.date(),
  updatedAt: z.coerce.date(),
});

export type Todo = z.infer<typeof todoSchema>;
export type CreateTodoInput = z.infer<typeof createTodoSchema>;
export type UpdateTodoInput = z.infer<typeof updateTodoSchema>;