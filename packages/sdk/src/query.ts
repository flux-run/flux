/**
 * @fluxbase/sdk — query builder & table client implementation.
 *
 * Each table client produced by `buildTableClient` maps friendly TypeScript
 * method calls to the Fluxbase query compiler's JSON DSL and posts them to
 * `POST /db/query` via the caller-supplied `fetcher` function.
 */

import type {
  Filter,
  OrderBy,
  QueryArgs,
  SelectFields,
  SelectResult,
  TableClient,
  TableChangeEvent,
  UnsubscribeFn,
} from "./types.js";
import { FluentQuery } from "./builder.js";

// ─── Streamer type ────────────────────────────────────────────────────────────

/**
 * @internal
 * A function that opens an SSE connection for `table`, optionally filtered
 * by `operation`, and calls `callback` for each matching event.
 * Returns an `UnsubscribeFn` that closes the connection.
 */
export type TableStreamer<T> = (
  args:     { operation?: "insert" | "update" | "delete" },
  callback: (event: TableChangeEvent<T>) => void,
) => UnsubscribeFn;

// ─── Internal response shapes ─────────────────────────────────────────────────

interface QueryResponse<T> {
  data?: T[];
  meta?: {
    rows: number;
    elapsed_ms: number;
    complexity?: number;
    strategy?: string;
    request_id?: string;
  };
  error?: string;
}

interface CountResponse {
  count: number;
}

// ─── Payload builders ─────────────────────────────────────────────────────────

function buildSelect<T>(
  table: string,
  args?: QueryArgs<T>,
): Record<string, unknown> {
  return {
    operation: "select",
    table,
    ...(args?.select   ? { select: normaliseSelect(args.select) } : {}),
    ...(args?.where    ? { where: args.where }                    : {}),
    ...(args?.limit    ? { limit: args.limit }                    : {}),
    ...(args?.offset   ? { offset: args.offset }                  : {}),
    ...(args?.order_by ? { order_by: args.order_by }              : {}),
  };
}

/**
 * Normalise a `SelectFields<T>` value to the format the query compiler
 * expects. Boolean `true` fields are kept; nested objects are recursed.
 */
function normaliseSelect<T>(
  fields: SelectFields<T>,
): Record<string, unknown> {
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

// ─── Factory ──────────────────────────────────────────────────────────────────

/**
 * Build a `TableClient<T, TInsert, TUpdate>` for `table` that uses the
 * provided `fetcher` to communicate with the Fluxbase Gateway.
 *
 * The `fetcher` is supplied by `createClient` and already has the
 * authentication headers baked in.
 *
 * The optional `streamer` is supplied by `createClient` for realtime
 * subscriptions (`subscribe()`). When omitted, calling `subscribe()` will
 * log a console warning and return a no-op unsubscribe function.
 */
export function buildTableClient<
  T,
  TInsert = Partial<T>,
  TUpdate = Partial<T>,
>(
  table: string,
  fetcher: (path: string, init?: RequestInit) => Promise<unknown>,
  streamer?: TableStreamer<T>,
): TableClient<T, TInsert, TUpdate> {
  return {
    // ── findMany ────────────────────────────────────────────────────────────
    async findMany<TSelect extends SelectFields<T> | undefined = undefined>(
      args?: Omit<QueryArgs<T>, "select"> & { select?: TSelect },
    ): Promise<Array<SelectResult<T, TSelect>>> {
      const body = JSON.stringify(buildSelect(table, args as QueryArgs<T>));
      const res = (await fetcher("/db/query", {
        method: "POST",
        body,
      })) as QueryResponse<unknown>;
      return (res.data ?? []) as unknown as Array<SelectResult<T, TSelect>>;
    },

    // ── findOne ─────────────────────────────────────────────────────────────
    async findOne<TSelect extends SelectFields<T> | undefined = undefined>(
      args?: Omit<QueryArgs<T>, "select"> & { select?: TSelect },
    ): Promise<SelectResult<T, TSelect> | null> {
      const payload = buildSelect(table, { ...(args as QueryArgs<T>), limit: 1 });
      const body = JSON.stringify(payload);
      const res = (await fetcher("/db/query", {
        method: "POST",
        body,
      })) as QueryResponse<unknown>;
      return ((res.data ?? [])[0] ?? null) as SelectResult<T, TSelect> | null;
    },

    // ── insert ──────────────────────────────────────────────────────────────
    async insert(data: TInsert | TInsert[]): Promise<T[]> {
      const rows = Array.isArray(data) ? data : [data];
      const body = JSON.stringify({ operation: "insert", table, rows });
      const res = (await fetcher("/db/query", {
        method: "POST",
        body,
      })) as QueryResponse<T>;
      return res.data ?? [];
    },

    // ── update ──────────────────────────────────────────────────────────────
    async update(where: Filter<T>, data: TUpdate): Promise<T[]> {
      const body = JSON.stringify({ operation: "update", table, where, data });
      const res = (await fetcher("/db/query", {
        method: "POST",
        body,
      })) as QueryResponse<T>;
      return res.data ?? [];
    },

    // ── delete ──────────────────────────────────────────────────────────────
    async delete(where: Filter<T>): Promise<{ deleted: number }> {
      const body = JSON.stringify({ operation: "delete", table, where });
      const res = (await fetcher("/db/query", {
        method: "POST",
        body,
      })) as { deleted?: number };
      return { deleted: res.deleted ?? 0 };
    },

    // ── count ───────────────────────────────────────────────────────────────
    async count(args?: Pick<QueryArgs<T>, "where">): Promise<number> {
      const body = JSON.stringify({
        operation: "count",
        table,
        ...(args?.where ? { where: args.where } : {}),
      });
      const res = (await fetcher("/db/query", {
        method: "POST",
        body,
      })) as CountResponse;
      return res.count ?? 0;
    },

    // ── subscribe (realtime SSE) ─────────────────────────────────────────────
    subscribe(
      args:     { operation?: "insert" | "update" | "delete" },
      callback: (event: TableChangeEvent<T>) => void,
    ): UnsubscribeFn {
      if (!streamer) {
        console.warn(
          `[fluxbase/sdk] subscribe() called on table "${table}" but no SSE ` +
          "streamer is configured. Make sure you are using createClient() " +
          "with a gateway URL that supports SSE.",
        );
        return () => { /* no-op */ };
      }
      return streamer(args, callback);
    },

    // ── query (fluent builder) ───────────────────────────────────────────────
    query(): FluentQuery<T, undefined> {
      return new FluentQuery<T, undefined>(table, fetcher, undefined, streamer);
    },
  };
}

// Re-export types consumed by consumers who import from `@fluxbase/sdk/query`.
export type { QueryArgs, Filter, OrderBy, SelectFields };
