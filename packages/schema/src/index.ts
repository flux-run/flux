// Runtime implementations (actual JS values)
export {
  defineEnum,
  defineSchema,
  column,
  index,
  foreignKey,
  ForbiddenError,
  ValidationError,
} from "./runtime.js";

// Type-only exports (erased at runtime)
export type {
  FluxEnum,
  ColumnDescriptor,
  InferRow,
  IndexDescriptor,
  ForeignKeyDescriptor,
  RuleCtx,
  HookCtx,
  RulePredicate,
  InsertPredicate,
  ColumnRules,
  SchemaRules,
  BeforeHooks,
  AfterHooks,
  OnHooks,
  SchemaHooks,
  SchemaDefinition,
  EventPayload,
  InterceptUpdate,
  JsonSchemaObject,
} from "./types.js";
