/**
 * @fluxbase/sdk — Fluent Query Builder
 *
 * Allows lazy query construction before execution.
 *
 * ```ts
 * const users = await flux.db.users
 *   .where("email", "eq", "a@b.com")
 *   .orderBy("created_at", "desc")
 *   .limit(10)
 *   .select({ id: true, email: true })
 *   .execute()
 *
 * // → Array<{ id: string; email: string }>
 * ```
 *
 * The builder is **lazy** — nothing is sent to the server until `.execute()`
 * (or `.first()` / `.count()`) is called. The same builder can be cloned and
 * extended without mutating the original.
 */

import type { Filter, FilterOp, SelectFields, SelectResult, OrderBy, TableChangeEvent, UnsubscribeFn } from "./types.js";
import type { TableStreamer } from "./query.js";

// ─── Internal state ────────────────────────────────────────────────────────────

interface BuilderState<T> {
  table:    string;
  where_:   Filter<T>;
  select_:  SelectFields<T> | undefined;
  orderBy_: OrderBy<T>;
  limit_:   number | undefined;
  offset_:  number | undefined;
}

// ─── FluentQuery ──────────────────────────────────────────────────────────────

/**
 * Fluent query builder that accumulates constraints and resolves them lazily.
 *
 * Instances are **immutable** — every chainable method returns a new instance.
 */
export class FluentQuery<
  T,
  TSelect extends SelectFields<T> | undefined = undefined,
> {
  /** @internal */
  readonly _state: BuilderState<T>;
  /** @internal */
  readonly _fetcher: (path: string, init?: RequestInit) => Promise<unknown>;
  /** @internal — optional SSE streamer supplied by createClient */
  readonly _streamer: TableStreamer<T> | undefined;

  /** @internal — use `buildTableClient` instead of constructing directly */
  constructor(
    table: string,
    fetcher: (path: string, init?: RequestInit) => Promise<unknown>,
    state?: Partial<BuilderState<T>>,
    streamer?: TableStreamer<T>,
  ) {
    this._fetcher = fetcher;
    this._streamer = streamer;
    this._state = {
      table,
      where_:   {},
      select_:  undefined,
      orderBy_: {},
      limit_:   undefined,
      offset_:  undefined,
      ...state,
    };
  }

  // ── Clone helper ─────────────────────────────────────────────────────────
  private clone<TNewSelect extends SelectFields<T> | undefined = TSelect>(
    patch: Partial<BuilderState<T>>,
  ): FluentQuery<T, TNewSelect> {
    return new FluentQuery<T, TNewSelect>(
      this._state.table,
      this._fetcher,
      { ...this._state, ...patch },
      this._streamer as TableStreamer<T> | undefined,
    );
  }

  // ── where ────────────────────────────────────────────────────────────────

  /**
   * Shorthand single-column filter:
   * ```ts
   * .where("status", "eq", "active")
   * .where("age",    "gte", 18)
   * .where("name",   "ilike", "%alice%")
   * ```
   *
   * Multiple calls are **AND**-composed — each call adds another constraint on
   * top of existing ones.
   */
  where<K extends keyof T>(
    column:   K,
    operator: keyof FilterOp<T[K]>,
    value:    NonNullable<T[K]>,
  ): FluentQuery<T, TSelect>;

  /**
   * Object filter — merges the provided `Filter<T>` into the existing where clause.
   * ```ts
   * .where({ status: { eq: "active" }, age: { gte: 18 } })
   * ```
   */
  where(filter: Filter<T>): FluentQuery<T, TSelect>;

  // Implementation
  where<K extends keyof T>(
    columnOrFilter: K | Filter<T>,
    operator?: keyof FilterOp<T[K]>,
    value?: NonNullable<T[K]>,
  ): FluentQuery<T, TSelect> {
    if (typeof columnOrFilter === "object") {
      return this.clone({
        where_: { ...this._state.where_, ...(columnOrFilter as Filter<T>) },
      });
    }

    const existing = (this._state.where_[columnOrFilter] ?? {}) as Record<string, unknown>;
    const updated: Filter<T> = {
      ...this._state.where_,
      [columnOrFilter]: {
        ...existing,
        [operator as string]: value,
      } as FilterOp<NonNullable<T[K]>>,
    };
    return this.clone({ where_: updated });
  }

  // ── select ───────────────────────────────────────────────────────────────

  /**
   * Pick the columns (and nested relations) to include in the response.
   * The return type of `.execute()` is automatically narrowed to match.
   *
   * ```ts
   * .select({ id: true, email: true, posts: { select: { title: true } } })
   * ```
   */
  select<S extends SelectFields<T>>(
    fields: S,
  ): FluentQuery<T, S> {
    return this.clone<S>({ select_: fields });
  }

  // ── orderBy ──────────────────────────────────────────────────────────────

  /**
   * Add an ordering constraint.
   * ```ts
   * .orderBy("created_at", "desc")
   * .orderBy("email", "asc")
   * ```
   * Multiple calls compose — later calls add secondary sort columns.
   */
  orderBy<K extends keyof T>(column: K, direction: "asc" | "desc" = "asc"): FluentQuery<T, TSelect> {
    return this.clone({
      orderBy_: { ...this._state.orderBy_, [column]: direction },
    });
  }

  // ── limit / offset ────────────────────────────────────────────────────────

  /** Maximum rows to return. */
  limit(n: number): FluentQuery<T, TSelect> {
    return this.clone({ limit_: n });
  }

  /** Skip the first `n` rows. */
  offset(n: number): FluentQuery<T, TSelect> {
    return this.clone({ offset_: n });
  }

  // ── Terminal methods ──────────────────────────────────────────────────────

  /**
   * Execute the query and return all matching rows.
   * The return type is narrowed by the `.select()` shape (if provided).
   */
  async execute(): Promise<Array<SelectResult<T, TSelect>>> {
    const payload = this._buildPayload("select");
    const res = (await this._fetcher("/db/query", {
      method: "POST",
      body:   JSON.stringify(payload),
    })) as { data?: unknown[] };
    return (res.data ?? []) as unknown as Array<SelectResult<T, TSelect>>;
  }

  /**
   * Execute and return only the first matching row (or `null`).
   */
  async first(): Promise<SelectResult<T, TSelect> | null> {
    const payload = this._buildPayload("select", { limit: 1 });
    const res = (await this._fetcher("/db/query", {
      method: "POST",
      body:   JSON.stringify(payload),
    })) as { data?: unknown[] };
    return ((res.data ?? [])[0] ?? null) as SelectResult<T, TSelect> | null;
  }

  /**
   * Count the number of rows matching the current `.where()` constraints.
   * Ignores `.select()`, `.limit()`, and `.offset()`.
   */
  async count(): Promise<number> {
    const payload = { operation: "count", table: this._state.table } as Record<string, unknown>;
    if (Object.keys(this._state.where_).length > 0) {
      payload.where = this._state.where_;
    }
    const res = (await this._fetcher("/db/query", {
      method: "POST",
      body:   JSON.stringify(payload),
    })) as { count?: number };
    return res.count ?? 0;
  }

  /**
   * Subscribe to realtime table-change events matching the current `.where()`
   * constraints (client-side filter applied where possible).
   *
   * Returns an unsubscribe function.
   *
   * ```ts
   * const unsub = flux.db.orders
   *   .query()
   *   .where("status", "eq", "pending")
   *   .subscribe({ operation: "insert" }, (event) => {
   *     console.log("New pending order:", event.row);
   *   });
   *
   * // stop listening:
   * unsub();
   * ```
   *
   * @note   Row-level `where()` filtering is applied client-side.
   *         Only `insert`, `update`, and `delete` events for this table are
   *         forwarded by the server; matching against the accumulated where
   *         clause is done locally.
   */
  subscribe(
    args:     { operation?: "insert" | "update" | "delete" },
    callback: (event: TableChangeEvent<T>) => void,
  ): UnsubscribeFn {
    if (!this._streamer) {
      console.warn(
        `[fluxbase/sdk] subscribe() called on table "${this._state.table}" ` +
        "but no SSE streamer is configured. " +
        "Make sure you are using createClient() with a gateway that supports SSE.",
      );
      return () => { /* no-op */ };
    }

    const whereKeys = Object.keys(this._state.where_);

    return this._streamer(args, (event) => {
      // Client-side where filter — skip rows that don't match.
      if (whereKeys.length > 0) {
        const row = event.row as Record<string, unknown>;
        const match = whereKeys.every((key) => {
          const filter = (this._state.where_ as Record<string, unknown>)[key];
          if (filter === null || typeof filter !== "object") {
            return row[key] === filter;
          }
          const f = filter as Record<string, unknown>;
          if ("eq"  in f) return row[key] === f["eq"];
          if ("neq" in f) return row[key] !== f["neq"];
          // Other operators (gt, gte, lt, lte, in, nin…) — pass through
          // to avoid false negatives; server sends the full row.
          return true;
        });
        if (!match) return;
      }
      callback(event);
    });
  }

  /**
   * Return the compiled query payload without executing it.
   * Useful for debugging or logging.
   */
  toPayload(): Record<string, unknown> {
    return this._buildPayload("select");
  }

  // ── Internal ──────────────────────────────────────────────────────────────

  private _buildPayload(
    operation: string,
    overrides?: { limit?: number; offset?: number },
  ): Record<string, unknown> {
    const { table, where_, select_, orderBy_, limit_, offset_ } = this._state;

    const payload: Record<string, unknown> = { operation, table };

    if (select_ && Object.keys(select_).length > 0) {
      payload.select = normaliseSelect(select_);
    }
    if (Object.keys(where_).length > 0)   payload.where    = where_;
    if (Object.keys(orderBy_).length > 0) payload.order_by = orderBy_;

    const resolvedLimit  = overrides?.limit  ?? limit_;
    const resolvedOffset = overrides?.offset ?? offset_;
    if (resolvedLimit  !== undefined) payload.limit  = resolvedLimit;
    if (resolvedOffset !== undefined) payload.offset = resolvedOffset;

    return payload;
  }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

function normaliseSelect<T>(fields: SelectFields<T>): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  for (const [key, val] of Object.entries(fields)) {
    if (val === true) {
      result[key] = true;
    } else if (val && typeof val === "object") {
      result[key] = { select: normaliseSelect(val as SelectFields<unknown>) };
    }
  }
  return result;
}

// ─── Type exports ─────────────────────────────────────────────────────────────

/** The fluent query builder type — the return type of `flux.db.users.query()` */
export type FluentQueryBuilder<T> = FluentQuery<T, undefined>;
