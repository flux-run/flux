import type { JsonSchemaObject } from "@flux/schema"

/**
 * Reusable JSONB schemas — import in *.schema.ts column.jsonb().schema(addressSchema)
 * Validated by the data engine at runtime before every insert/update.
 */

export const addressSchema: JsonSchemaObject = {
  type: "object",
  required: ["street", "city", "country"],
  additionalProperties: false,
  properties: {
    street:     { type: "string", maxLength: 200 },
    city:       { type: "string", maxLength: 100 },
    state:      { type: "string", maxLength: 100 },
    country:    { type: "string", minLength: 2, maxLength: 2 },
    zip:        { type: "string", pattern: "^[0-9A-Z\\-]{3,10}$" },
    is_primary: { type: "boolean", default: false },
  },
}

export const moneySchema: JsonSchemaObject = {
  type: "object",
  required: ["amount", "currency"],
  additionalProperties: false,
  properties: {
    amount:   { type: "number", minimum: 0 },
    currency: { type: "string", minLength: 3, maxLength: 3 },
  },
}

export const mediaSchema: JsonSchemaObject = {
  type: "object",
  required: ["url", "type"],
  additionalProperties: false,
  properties: {
    url:      { type: "string" },
    type:     { type: "string", enum: ["image", "video", "audio", "document"] },
    filename: { type: "string" },
    size:     { type: "number", minimum: 0 },
    mime:     { type: "string" },
  },
}

export const addressListSchema: JsonSchemaObject = {
  type: "array",
  maxItems: 10,
  items: addressSchema,
}
