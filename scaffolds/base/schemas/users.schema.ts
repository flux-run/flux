import { defineSchema, column, index, foreignKey, ForbiddenError } from "@fluxbase/schema"
import { userRole, userStatus } from "./_types"
import { adminOrOwner, adminOnly, adminOrMod } from "./_shared/auth"
import { addressListSchema, moneySchema } from "./_shared/jsonb"

/**
 * users table — demonstrates all schema features:
 *   columns    : all types including enum, jsonb, arrays
 *   indexes    : unique, composite, GIN
 *   foreignKeys: with on_delete/on_update
 *   rules      : row-level + column-level authorization (compiled to AST → Rust)
 *   hooks      : before/after transforms + async events via queue
 *
 * Run `flux db push` to apply to Postgres.
 */
export default defineSchema({
  table:       "users",
  description: "Application users",
  timestamps:  true,   // auto-adds created_at + updated_at with trigger

  // ── Columns ───────────────────────────────────────────────────────
  // Row type (UsersRow) is inferred from these — no separate interface needed.

  columns: {
    id:              column.uuid().primaryKey().default("gen_random_uuid()"),
    email:           column.text().unique().notNull(),
    name:            column.text().notNull(),
    age:             column.int().nullable().check("age >= 0 AND age <= 150"),
    balance:         column.numeric(10, 2).notNull().default("0.00"),
    is_verified:     column.boolean().notNull().default(false),
    login_count:     column.bigint().notNull().default(0),
    organization_id: column.uuid().nullable(),   // FK declared below
    role:            column.enum(userRole).default("user"),
    status:          column.enum(userStatus).default("pending"),
    tags:            column.array("text").nullable(),
    password_hash:   column.text().notNull(),

    // jsonb column with inline JSON Schema validation
    metadata: column.jsonb().nullable().schema({
      type: "object",
      additionalProperties: false,
      properties: {
        theme:         { type: "string", enum: ["light", "dark", "system"] },
        locale:        { type: "string", pattern: "^[a-z]{2}(-[A-Z]{2})?$" },
        notifications: { type: "boolean" },
      },
    }),

    // jsonb array — reusing shared schema from _shared/jsonb.ts
    addresses: column.jsonb().nullable().schema(addressListSchema),

    // nested jsonb object
    billing: column.jsonb().nullable().schema(moneySchema),
  },

  // ── Indexes ────────────────────────────────────────────────────────

  indexes: [
    index(["email"]).unique(),
    index(["organization_id", "status"]).name("idx_users_org_status"),
    index(["metadata"]).gin().name("idx_users_metadata_gin"),
    index(["tags"]).gin().name("idx_users_tags_gin"),
  ],

  // ── Foreign Keys ───────────────────────────────────────────────────

  foreignKeys: [
    foreignKey(["organization_id"])
      .references("organizations.id")
      .onDelete("set_null")
      .onUpdate("cascade"),
  ],

  // ── Rules — compiled to RuleExpr AST by flux db push ──────────────
  // Evaluated by Rust at runtime BEFORE any SQL is issued.
  // Import shared predicates from _shared/auth.ts — compiler inlines them.

  rules: {
    read:   adminOrOwner,
    insert: adminOrMod,
    update: adminOrOwner,
    delete: adminOnly,

    columns: {
      // password_hash is NEVER returned in any response
      password_hash: { read: () => false },

      // balance: visible to owner/admin, writable only by admin
      balance: {
        read:  adminOrOwner,
        write: adminOnly,
      },

      // role: only admins can promote/demote
      role:   { write: adminOnly },
    },
  },

  // ── Hooks — before/after + async events ───────────────────────────
  // Simple transforms → compiled to TransformExpr AST → evaluated by Rust
  // Complex logic (if/throw) → compiled to WASM → evaluated by Wasmtime
  // on.* → function refs pushed to queue (non-blocking, fully traced)

  hooks: {
    before: {
      insert: ({ input, ctx }) => ({
        ...input,
        email:      input.email?.toLowerCase(),   // normalize email
        created_by: ctx.user.id,
      }),

      update: ({ input, ctx }) => ({
        ...input,
        updated_by: ctx.user.id,
      }),

      delete: ({ row }) => {
        // Prevent hard-deleting admins
        if (row.role === "admin") {
          throw new ForbiddenError("Admin accounts cannot be deleted. Suspend instead.")
        }
        // Soft delete: intercept DELETE → UPDATE
        return { intercept: "update", data: { status: "deleted" } }
      },
    },

    after: {
      // Add computed field to every read response
      read: ({ rows }) => rows.map(row => ({
        ...row,
        display_name: `${row.name} <${row.email}>`,
      })),
    },

    // Async events — payload: EventPayload<UsersRow> (auto-injected by data engine)
    on: {
      insert: ["send_welcome_email", "sync_to_crm"],

      // Conditional: only fire when status actually changed
      update: ({ row, input }) =>
        input.status !== undefined && input.status !== row.status
          ? ["on_user_status_changed"]
          : [],

      delete: ["cleanup_user_files", "revoke_sessions"],
    },
  },
})
