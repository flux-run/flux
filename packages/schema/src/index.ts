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
  SchemaDefinition,
  JsonSchemaObject,
} from "./types.js";
