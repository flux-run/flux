import { z } from "npm:zod";

export const createDispatchSchema = z.object({
  orderId: z.string().min(1).max(128),
  message: z.string().min(1).max(2000),
});

export const dispatchRecordSchema = z.object({
  id: z.number().int(),
  orderId: z.string(),
  message: z.string(),
  status: z.enum(["pending", "delivered"]),
  remoteStatus: z.number().int().nullable(),
  createdAt: z.string(),
  deliveredAt: z.string().nullable(),
});

export type CreateDispatchInput = z.infer<typeof createDispatchSchema>;
export type DispatchRecord = z.infer<typeof dispatchRecordSchema>;