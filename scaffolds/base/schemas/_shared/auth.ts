import type { RulePredicate, InsertPredicate } from "@fluxbase/schema"

/**
 * Reusable rule predicates — import in any *.schema.ts
 * `flux db push` inlines the function body into the RuleExpr AST.
 *
 * Supported operations (compiled to AST, evaluated by Rust at runtime):
 *   ===  !==  &&  ||  !  > < >= <=  .includes()  == null  != null
 */

/** Allow if the requester is an admin OR owns the row */
export const adminOrOwner: RulePredicate<{ id: string }> =
  ({ ctx, row }) => ctx.user.role === "admin" || ctx.user.id === row.id

/** Allow admins only */
export const adminOnly: RulePredicate<unknown> =
  ({ ctx }) => ctx.user.role === "admin"

/** Allow admins and moderators */
export const adminOrMod: RulePredicate<unknown> =
  ({ ctx }) => ["admin", "moderator"].includes(ctx.user.role)

/** Allow any authenticated user (user.id is set) */
export const authenticated: InsertPredicate<unknown> =
  ({ ctx }) => ctx.user.id !== null

/** Deny everyone */
export const denyAll: RulePredicate<unknown> =
  () => false

/** Allow everyone (public) */
export const allowAll: RulePredicate<unknown> =
  () => true
