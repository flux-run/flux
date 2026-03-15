/**
 * @flux/schema — runtime implementation
 *
 * These functions return plain objects at runtime.
 * The TypeScript type system enforces correctness at author-time.
 * `flux db push` uses Deno to import *.schema.ts and inspect the return value.
 */

// ── defineEnum ────────────────────────────────────────────────────────────────

export function defineEnum<const T extends readonly string[]>(
  values: T,
): { readonly __type: "enum"; readonly values: T } {
  return { __type: "enum", values };
}

// ── defineSchema ──────────────────────────────────────────────────────────────
// Identity function — returns the config as-is so the compiler can read it.

export function defineSchema(config: Record<string, unknown>): typeof config {
  return config;
}

// ── column builder ────────────────────────────────────────────────────────────
// Each method returns a descriptor object; methods are chainable via Proxy.

function makeColumn(base: Record<string, unknown>) {
  const descriptor: Record<string, unknown> = { ...base };

  const proxy: Record<string, unknown> = new Proxy(descriptor, {
    get(target, prop) {
      if (prop === "toJSON" || prop === "then") return undefined;
      if (prop in target) return target[prop as string];
      // Fluent builder methods — record the option and return self
      return (...args: unknown[]) => {
        if (prop === "default")     { target["default"] = args[0]; return proxy; }
        if (prop === "notNull")     { target["not_null"] = true; return proxy; }
        if (prop === "primaryKey")  { target["primary_key"] = true; return proxy; }
        if (prop === "unique")      { target["unique"] = true; return proxy; }
        if (prop === "nullable")    { target["not_null"] = false; return proxy; }
        if (prop === "schema")      { target["jsonb_schema"] = args[0]; return proxy; }
        if (prop === "read")        { target["read"] = args[0]; return proxy; }
        if (prop === "write")       { target["write"] = args[0]; return proxy; }
        if (prop === "check")       { target["check"] = args[0]; return proxy; }
        // Unknown method — record it
        target[prop as string] = args.length === 1 ? args[0] : args;
        return proxy;
      };
    },
  });

  return proxy;
}

export const column = {
  uuid:        () => makeColumn({ type: "uuid" }),
  text:        () => makeColumn({ type: "text" }),
  varchar:     (n?: number) => makeColumn({ type: "varchar", length: n }),
  int:         () => makeColumn({ type: "int" }),
  bigint:      () => makeColumn({ type: "bigint" }),
  float:       () => makeColumn({ type: "float" }),
  numeric:     (p?: number, s?: number) => makeColumn({ type: "numeric", precision: p, scale: s }),
  boolean:     () => makeColumn({ type: "boolean" }),
  timestamptz: () => makeColumn({ type: "timestamptz" }),
  date:        () => makeColumn({ type: "date" }),
  time:        () => makeColumn({ type: "time" }),
  bytea:       () => makeColumn({ type: "bytea" }),
  jsonb:       () => makeColumn({ type: "jsonb" }),
  json:        () => makeColumn({ type: "json" }),
  enum:        (e: { values: readonly string[] }) => makeColumn({ type: "enum", enum_values: e.values }),
  array:       (inner: unknown) => makeColumn({ type: "array", of: inner }),
  serial:      () => makeColumn({ type: "serial" }),
  bigserial:   () => makeColumn({ type: "bigserial" }),
};

// ── index ─────────────────────────────────────────────────────────────────────

export function index(columns: string[]): Record<string, unknown> {
  const descriptor: Record<string, unknown> = { __type: "index", columns };
  const proxy: Record<string, unknown> = new Proxy(descriptor, {
    get(target, prop) {
      if (prop === "toJSON" || prop === "then") return undefined;
      if (prop in target) return target[prop as string];
      return (...args: unknown[]) => {
        if (prop === "unique") { target["unique"] = true; return proxy; }
        if (prop === "gin")    { target["type"] = "gin"; return proxy; }
        if (prop === "btree")  { target["type"] = "btree"; return proxy; }
        if (prop === "name")   { target["name"] = args[0]; return proxy; }
        target[prop as string] = args.length === 1 ? args[0] : args;
        return proxy;
      };
    },
  });
  return proxy;
}

// ── foreignKey ────────────────────────────────────────────────────────────────

export function foreignKey(columns: string[]): Record<string, unknown> {
  const descriptor: Record<string, unknown> = { __type: "fk", columns };
  const proxy: Record<string, unknown> = new Proxy(descriptor, {
    get(target, prop) {
      if (prop === "toJSON" || prop === "then") return undefined;
      if (prop in target) return target[prop as string];
      return (...args: unknown[]) => {
        if (prop === "references") { target["references"] = args[0]; return proxy; }
        if (prop === "onDelete")   { target["on_delete"] = args[0]; return proxy; }
        if (prop === "onUpdate")   { target["on_update"] = args[0]; return proxy; }
        target[prop as string] = args.length === 1 ? args[0] : args;
        return proxy;
      };
    },
  });
  return proxy;
}

// ── Error helpers (available in rules/hooks) ──────────────────────────────────

export class ForbiddenError extends Error {
  constructor(message = "Forbidden") { super(message); this.name = "ForbiddenError"; }
}

export class ValidationError extends Error {
  constructor(message = "Validation failed") { super(message); this.name = "ValidationError"; }
}
