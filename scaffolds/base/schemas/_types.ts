import { defineEnum } from "@fluxbase/schema"

// Shared Postgres enum types — referenced in *.schema.ts as column.enum(userRole)
// `flux db push` creates these as native Postgres enum types.

export const userRole = defineEnum("user_role", [
  "guest", "user", "moderator", "admin",
] as const)

export const userStatus = defineEnum("user_status", [
  "pending", "active", "suspended", "deleted",
] as const)

export const currencyCode = defineEnum("currency_code", [
  "USD", "EUR", "GBP", "JPY", "INR", "AUD", "CAD",
] as const)

export const paymentStatus = defineEnum("payment_status", [
  "pending", "processing", "completed", "failed", "refunded",
] as const)

export const mediaType = defineEnum("media_type", [
  "image", "video", "audio", "document", "other",
] as const)
