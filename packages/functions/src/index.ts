/**
 * @fluxbase/functions — Fluxbase serverless function framework
 *
 * Developers use `defineFunction` to create schema-validated, typed serverless
 * functions. The runtime calls `functionDef.execute(payload, context)`.
 */

export type {
  Schema,
  FluxContext,
  FluxSecrets,
  FluxQueue,
  FluxFunction,
  FluxFetch,
  FluxDb,
  FluxTools,
  FluxWorkflow,
  FluxContext as Ctx,
  HandlerArgs,
  DefineFunctionOptions,
  FunctionDefinition,
} from "./types.js";

import type {
  Schema,
  FluxContext,
  DefineFunctionOptions,
  FunctionDefinition,
} from "./types.js";

// ─── Schema helpers ───────────────────────────────────────────────────────────

/**
 * Attempt to extract a JSON Schema-like object from a Zod schema.
 * This is a best-effort extraction for metadata purposes only — the actual
 * runtime validation is done by Zod's `.parse()` method.
 */
function extractJsonSchema(
  schema: Schema | undefined,
): Record<string, unknown> | null {
  if (!schema) return null;

  // Zod v3 exposes _def which we can walk to produce a lightweight JSON Schema
  const def = (schema as unknown as Record<string, unknown>)["_def"];
  if (!def) return null;

  try {
    return walkZodDef(def as Record<string, unknown>);
  } catch {
    return { type: "object", description: "Schema extraction failed" };
  }
}

function walkZodDef(def: Record<string, unknown>): Record<string, unknown> {
  const typeName = def["typeName"] as string | undefined;

  switch (typeName) {
    case "ZodString":
      return { type: "string" };
    case "ZodNumber":
      return { type: "number" };
    case "ZodBoolean":
      return { type: "boolean" };
    case "ZodNull":
      return { type: "null" };
    case "ZodUndefined":
      return { type: "undefined" };
    case "ZodAny":
      return {};
    case "ZodUnknown":
      return {};

    case "ZodOptional": {
      const inner = walkZodDef(
        (def["innerType"] as Record<string, unknown>)["_def"] as Record<
          string,
          unknown
        >,
      );
      return { ...inner, optional: true };
    }

    case "ZodNullable": {
      const inner = walkZodDef(
        (def["innerType"] as Record<string, unknown>)["_def"] as Record<
          string,
          unknown
        >,
      );
      return { anyOf: [inner, { type: "null" }] };
    }

    case "ZodArray": {
      const items = walkZodDef(
        (def["type"] as Record<string, unknown>)["_def"] as Record<
          string,
          unknown
        >,
      );
      return { type: "array", items };
    }

    case "ZodObject": {
      const shape = def["shape"] as
        | (() => Record<string, unknown>)
        | Record<string, unknown>;
      const resolvedShape = typeof shape === "function" ? shape() : shape;
      const properties: Record<string, unknown> = {};
      const required: string[] = [];

      for (const [key, value] of Object.entries(resolvedShape)) {
        const fieldDef = (value as Record<string, unknown>)["_def"] as Record<
          string,
          unknown
        >;
        properties[key] = walkZodDef(fieldDef);
        if (fieldDef["typeName"] !== "ZodOptional") {
          required.push(key);
        }
      }

      return {
        type: "object",
        properties,
        required: required.length ? required : undefined,
      };
    }

    case "ZodEnum": {
      const values = def["values"] as string[];
      return { type: "string", enum: values };
    }

    case "ZodUnion": {
      const options = (def["options"] as Array<Record<string, unknown>>).map(
        (o) => walkZodDef(o["_def"] as Record<string, unknown>),
      );
      return { anyOf: options };
    }

    case "ZodLiteral": {
      return { const: def["value"] };
    }

    default:
      return { type: "unknown", zodType: typeName };
  }
}

// ─── defineFunction ───────────────────────────────────────────────────────────

/**
 * Define a Fluxbase serverless function.
 *
 * @example
 * ```ts
 * import { defineFunction } from "@fluxbase/functions"
 * import { z } from "zod"
 *
 * export default defineFunction({
 *   name: "hello",
 *   input: z.object({ name: z.string() }),
 *   handler: async ({ input }) => ({ message: `Hello ${input.name}` })
 * })
 * ```
 */
export function defineFunction<TInput = unknown, TOutput = unknown>(
  options: DefineFunctionOptions<TInput, TOutput>,
): FunctionDefinition<TInput, TOutput> {
  const {
    name,
    description,
    input: inputSchema,
    output: outputSchema,
    handler,
  } = options;

  // Extract JSON Schema representations for metadata storage (best-effort)
  const input_schema = extractJsonSchema(inputSchema as Schema | undefined);
  const output_schema = extractJsonSchema(outputSchema as Schema | undefined);

  const definition: FunctionDefinition<TInput, TOutput> = {
    __fluxbase: true,

    metadata: {
      name,
      description,
      input_schema,
      output_schema,
    },

    async execute(payload: unknown, context: FluxContext): Promise<TOutput> {
      // 1. Validate input
      let input: TInput;
      if (inputSchema) {
        const result = inputSchema.safeParse(payload);
        if (!result.success) {
          throw Object.assign(
            new Error("Invalid function input: schema validation failed"),
            { code: "INPUT_VALIDATION_ERROR", details: result.error },
          );
        }
        input = result.data as TInput;
      } else {
        input = payload as TInput;
      }

      // 2. Execute handler
      const output = await handler({ input, ctx: context });

      // 3. Validate output
      if (outputSchema) {
        const result = outputSchema.safeParse(output);
        if (!result.success) {
          throw Object.assign(
            new Error("Invalid function output: schema validation failed"),
            { code: "OUTPUT_VALIDATION_ERROR", details: result.error },
          );
        }
      }

      return output;
    },
  };

  return definition;
}
