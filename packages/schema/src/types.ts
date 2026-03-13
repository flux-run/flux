/**
 * @fluxbase/schema — Type definitions for Flux schema definitions.
 *
 * One `*.schema.ts` file per DB table. Contains columns, indexes, foreign keys,
 * authorization rules, and lifecycle hooks all in one place.
 *
 * Processed by `flux db push`:
 *   columns/indexes/FK  → Postgres DDL (diff + migrate)
 *   rules               → RuleExpr AST JSON → stored in flux.schema_rules → evaluated by Rust
 *   hooks.before/after  → TransformExpr AST or WASM → evaluated by Rust/Wasmtime
 *   hooks.on            → function ref list → pushed to queue
 */

// ── Enums ────────────────────────────────────────────────────────────────────

export interface FluxEnum<Values extends readonly string[]> {
  readonly __fluxEnum: true;
  readonly name: string;
  readonly values: Values;
}

export function defineEnum<const Values extends readonly string[]>(
  name: string,
  values: Values,
): FluxEnum<Values> {
  return { __fluxEnum: true, name, values };
}

// ── Column builder ────────────────────────────────────────────────────────────

/** Internal column descriptor — not used directly by developers */
export interface ColumnDescriptor<T> {
  readonly __fluxColumn: true;
  readonly pgType: string;
  readonly nullable: boolean;
  readonly _type: T; // phantom type for inference
  primaryKey(): ColumnDescriptor<T>;
  notNull(): ColumnDescriptor<NonNullable<T>>;
  nullable(): ColumnDescriptor<T | null>;
  unique(): ColumnDescriptor<T>;
  default(value: string | number | boolean): ColumnDescriptor<T>;
  check(expr: string): ColumnDescriptor<T>;
  schema(jsonSchema: JsonSchemaObject): ColumnDescriptor<T>; // jsonb only
}

// Column type inference helpers
type Infer<C> = C extends ColumnDescriptor<infer T> ? T : never;
export type InferRow<Cols extends Record<string, ColumnDescriptor<unknown>>> = {
  [K in keyof Cols]: Infer<Cols[K]>;
};

/** Numeric column with precision + scale */
interface NumericBuilder extends ColumnDescriptor<number> {
  precision(p: number, s: number): NumericBuilder;
}

/** Array column */
interface ArrayBuilder<T> extends ColumnDescriptor<T[]> {
  nullable(): ArrayBuilder<T[] | null>;
}

/** JSONB column with optional schema validation */
interface JsonbBuilder<T = unknown> extends ColumnDescriptor<T> {
  schema<S>(jsonSchema: JsonSchemaObject): JsonbBuilder<S>;
  nullable(): JsonbBuilder<T | null>;
}

/** The `column` builder namespace */
export declare const column: {
  uuid(): ColumnDescriptor<string>;
  text(): ColumnDescriptor<string>;
  int(): ColumnDescriptor<number>;
  bigint(): ColumnDescriptor<number>;
  float(): ColumnDescriptor<number>;
  numeric(precision?: number, scale?: number): NumericBuilder;
  boolean(): ColumnDescriptor<boolean>;
  timestamptz(): ColumnDescriptor<string>;
  date(): ColumnDescriptor<string>;
  bytea(): ColumnDescriptor<Uint8Array>;
  jsonb<T = unknown>(): JsonbBuilder<T>;
  array(elementType: "text" | "int" | "uuid" | "float" | "boolean"): ArrayBuilder<unknown>;
  enum<E extends FluxEnum<readonly string[]>>(e: E): ColumnDescriptor<E["values"][number]>;
};

// ── Index builder ─────────────────────────────────────────────────────────────

export interface IndexDescriptor {
  readonly columns: string[];
  unique(): IndexDescriptor;
  name(n: string): IndexDescriptor;
  gin(): IndexDescriptor;       // for jsonb / array columns
  btree(): IndexDescriptor;     // default
}

export declare function index(columns: string[]): IndexDescriptor;

// ── Foreign key builder ───────────────────────────────────────────────────────

type FKAction = "restrict" | "cascade" | "set_null" | "no_action";

export interface ForeignKeyDescriptor {
  readonly columns: string[];
  references(tableAndColumn: `${string}.${string}`): ForeignKeyDescriptor;
  onDelete(action: FKAction): ForeignKeyDescriptor;
  onUpdate(action: FKAction): ForeignKeyDescriptor;
}

export declare function foreignKey(columns: string[]): ForeignKeyDescriptor;

// ── Rule expressions ──────────────────────────────────────────────────────────

/** Context available inside rule expressions */
export interface RuleCtx {
  user: {
    id: string;
    role: string;
    [claim: string]: unknown;
  };
  request: {
    ip: string;
    headers: Record<string, string>;
  };
}

/** A rule predicate — compiled to RuleExpr AST by flux db push */
export type RulePredicate<Row> = (args: { ctx: RuleCtx; row: Row }) => boolean;
export type InsertPredicate<Row> = (args: { ctx: RuleCtx; input: Partial<Row> }) => boolean;

export interface ColumnRules<Row> {
  read?: RulePredicate<Row> | (() => boolean);
  write?: RulePredicate<Row> | (() => boolean);
}

export interface SchemaRules<Row> {
  read?:   RulePredicate<Row>;
  insert?: InsertPredicate<Row>;
  update?: RulePredicate<Row>;
  delete?: RulePredicate<Row>;
  columns?: {
    [K in keyof Row]?: ColumnRules<Row>;
  };
}

// ── Hook types ────────────────────────────────────────────────────────────────

/** Intercept a delete and turn it into an update (soft delete) */
export interface InterceptUpdate<Row> {
  intercept: "update";
  data: Partial<Row>;
}

export class ForbiddenError extends Error {
  constructor(message: string) { super(message); this.name = "ForbiddenError"; }
}
export class ValidationError extends Error {
  constructor(message: string) { super(message); this.name = "ValidationError"; }
}

export interface HookCtx extends RuleCtx {
  request_id: string;
  timestamp:  string;
}

export interface BeforeHooks<Row> {
  insert?: (args: { input: Partial<Row>; ctx: HookCtx }) => Partial<Row> | Promise<Partial<Row>>;
  update?: (args: { input: Partial<Row>; row: Row; ctx: HookCtx }) => Partial<Row> | Promise<Partial<Row>>;
  delete?: (args: { row: Row; ctx: HookCtx }) => void | InterceptUpdate<Row> | Promise<void | InterceptUpdate<Row>>;
  read?:   (args: { ctx: HookCtx }) => void | Promise<void>;
}

export interface AfterHooks<Row> {
  insert?: (args: { row: Row; ctx: HookCtx }) => Row | Promise<Row>;
  update?: (args: { row: Row; prev: Row; ctx: HookCtx }) => Row | Promise<Row>;
  delete?: (args: { row: Row; ctx: HookCtx }) => void | Promise<void>;
  read?:   (args: { rows: Row[]; ctx: HookCtx }) => Row[] | Promise<Row[]>;
}

/** Event payload auto-injected into on: function handlers */
export interface EventPayload<Row> {
  operation:  "insert" | "update" | "delete";
  table:      string;
  row:        Row;
  prev:       Row | null;
  input:      Partial<Row>;
  ctx: {
    user:       { id: string; role: string; [key: string]: unknown };
    request_id: string;
    ip:         string;
    timestamp:  string;
  };
}

type EventFn<Row> = string[] | ((args: { row: Row; input: Partial<Row>; prev?: Row }) => string[]);

export interface OnHooks<Row> {
  insert?: EventFn<Row>;
  update?: EventFn<Row>;
  delete?: EventFn<Row>;
}

export interface SchemaHooks<Row> {
  before?: BeforeHooks<Row>;
  after?:  AfterHooks<Row>;
  on?:     OnHooks<Row>;
}

// ── defineSchema ──────────────────────────────────────────────────────────────

export interface SchemaDefinition<
  Cols extends Record<string, ColumnDescriptor<unknown>>,
> {
  table:        string;
  description?: string;
  timestamps?:  boolean;           // adds created_at + updated_at
  columns:      Cols;
  indexes?:     IndexDescriptor[];
  foreignKeys?: ForeignKeyDescriptor[];
  rules?:       SchemaRules<InferRow<Cols>>;
  hooks?:       SchemaHooks<InferRow<Cols>>;
}

export declare function defineSchema<
  Cols extends Record<string, ColumnDescriptor<unknown>>,
>(definition: SchemaDefinition<Cols>): SchemaDefinition<Cols>;

// ── JSON Schema types (for .schema() on jsonb columns) ───────────────────────

export interface JsonSchemaObject {
  type?: "object" | "array" | "string" | "number" | "integer" | "boolean" | "null";
  properties?: Record<string, JsonSchemaObject>;
  items?: JsonSchemaObject;
  required?: string[];
  additionalProperties?: boolean | JsonSchemaObject;
  enum?: unknown[];
  pattern?: string;
  minLength?: number;
  maxLength?: number;
  minimum?: number;
  maximum?: number;
  minItems?: number;
  maxItems?: number;
  default?: unknown;
  $ref?: string;
}
