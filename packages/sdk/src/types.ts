/**
 * @fluxbase/sdk — core type definitions.
 *
 * The `FluxbaseDB` and `FluxbaseFunctions` interfaces are intentionally empty
 * here. They are augmented by the auto-generated SDK file produced by
 * calling `GET /sdk/typescript` on your Fluxbase API, which inserts a typed
 * entry per table and per function.
 *
 * Example generated augmentation:
 *
 * ```ts
 * declare module "@fluxbase/sdk" {
 *   interface FluxbaseDB {
 *     users: TableClient<User, InsertUser, UpdateUser>;
 *   }
 *   interface FluxbaseFunctions {
 *     send_email(input: SendEmailInput): Promise<SendEmailOutput>;
 *   }
 * }
 * ```
 */

// ─── File storage ─────────────────────────────────────────────────────────────

export type FluxFile = {
  /** Public CDN URL */
  url: string;
  /** Internal storage key */
  key: string;
  /** File size in bytes */
  size: number;
  /** MIME type */
  mime_type: string;
};

// ─── Query building ───────────────────────────────────────────────────────────

/** Nested select — mirrors the Fluxbase query compiler's select syntax. */
export type SelectFields<T> = {
  [K in keyof T]?: boolean | (NonNullable<T[K]> extends object ? SelectFields<NonNullable<T[K]>> : never);
};

/** Comparison operators for filtering. */
export interface FilterOp<T> {
  eq?:     T;
  neq?:    T;
  gt?:     T;
  gte?:    T;
  lt?:     T;
  lte?:    T;
  like?:   string;
  ilike?:  string;
  in?:     T[];
  nin?:    T[];
  is_null?: boolean;
}

/** Row-level filter: each key can be an exact value or a FilterOp. */
export type Filter<T> = {
  [K in keyof T]?: T[K] | FilterOp<NonNullable<T[K]>>;
};

/** Column ordering direction. */
export type OrderBy<T> = {
  [K in keyof T]?: "asc" | "desc";
};

/** Standard query arguments accepted by `findMany` and `findOne`. */
export interface QueryArgs<T> {
  select?:   SelectFields<T>;
  where?:    Filter<T>;
  limit?:    number;
  offset?:   number;
  order_by?: OrderBy<T>;
}

// ─── Phase 5: Select result inference ────────────────────────────────────────

/** Extract the element type from an array type (or the type itself if not an array). */
type ArrayItem<T> = T extends ReadonlyArray<infer U> ? U : T;

/**
 * Infer the return type from a select shape — just like Prisma's select.
 *
 * - If `S` is `undefined` (no select provided), returns `T` unchanged.
 * - If `S` names specific fields, returns an object with *only* those fields.
 * - Nested selects recurse into relationship arrays and single objects.
 *
 * @example
 * // flux.db.users.findMany({ select: { id: true, email: true } })
 * // → Promise<Array<{ id: string; email: string }>>
 *
 * @example
 * // flux.db.posts.findMany({
 * //   select: { title: true, author: { select: { email: true } } }
 * // })
 * // → Promise<Array<{ title: string; author?: { email: string } }>>
 */
export type SelectResult<
  T,
  S extends SelectFields<T> | undefined = undefined,
> = [S] extends [undefined]
  ? T
  : {
      [K in Extract<keyof NonNullable<S>, keyof T>]-?: NonNullable<S>[K] extends true
        ? T[K]
        : NonNullable<T[K]> extends ReadonlyArray<unknown>
        ? NonNullable<S>[K] extends SelectFields<ArrayItem<NonNullable<T[K]>>>
          ? Array<
              SelectResult<
                ArrayItem<NonNullable<T[K]>>,
                NonNullable<S>[K] & SelectFields<ArrayItem<NonNullable<T[K]>>>
              >
            >
            | (T[K] extends null ? null : never)
            | (T[K] extends undefined ? undefined : never)
          : T[K]
        : NonNullable<S>[K] extends SelectFields<NonNullable<T[K]>>
        ? SelectResult<
            NonNullable<T[K]>,
            NonNullable<S>[K] & SelectFields<NonNullable<T[K]>>
          >
          | (T[K] extends null ? null : never)
          | (T[K] extends undefined ? undefined : never)
        : T[K];
    };

// ─── Nested relation helpers ──────────────────────────────────────────────────

/**
 * Relation connect helper — mirrors Prisma's `{ connect: { id } }` pattern.
 * Used in generated `Insert<T>` types for outgoing foreign-key relationships.
 *
 * The generated SDK file produces:
 * ```ts
 * export type InsertPost = Omit<Post, "id" | "created_at"> & {
 *   author?: Connect<User, "id">;   //  { connect: { id: string } }
 * };
 * ```
 */
export type Connect<T, K extends keyof T = "id" extends keyof T ? "id" : keyof T> = {
  connect: Pick<T, K>;
};

// ─── Table client ─────────────────────────────────────────────────────────────

/**
 * Typed table client interface.
 *
 * `T`       — full row type (all columns)
 * `TInsert` — insert payload (Omit<T, auto_fields> & connect helpers)
 * `TUpdate` — update payload (typically Partial<TInsert>)
 *
 * The `findMany` and `findOne` methods are generic on `TSelect` so that
 * TypeScript narrows the return type to exactly the requested fields.
 */
export interface TableClient<T, TInsert = Partial<T>, TUpdate = Partial<T>> {
  /** Return rows matching `args`. Return type is narrowed to `select` shape. */
  findMany<TSelect extends SelectFields<T> | undefined = undefined>(
    args?: Omit<QueryArgs<T>, "select"> & { select?: TSelect },
  ): Promise<Array<SelectResult<T, TSelect>>>;

  /** Return the first matching row or null. Return type is narrowed to `select` shape. */
  findOne<TSelect extends SelectFields<T> | undefined = undefined>(
    args?: Omit<QueryArgs<T>, "select"> & { select?: TSelect },
  ): Promise<SelectResult<T, TSelect> | null>;

  /** Insert one or many rows; returns the inserted rows (full type). */
  insert(data: TInsert | TInsert[]): Promise<T[]>;

  /** Update rows matching `where`; returns the updated rows (full type). */
  update(where: Filter<T>, data: TUpdate): Promise<T[]>;

  /** Delete rows matching `where`. */
  delete(where: Filter<T>): Promise<{ deleted: number }>;

  /** Count rows matching `where`. */
  count(args?: Pick<QueryArgs<T>, "where">): Promise<number>;

  /**
   * Start a fluent query builder for this table.
   *
   * ```ts
   * const users = await flux.db.users
   *   .query()
   *   .where("active", "eq", true)
   *   .orderBy("created_at", "desc")
   *   .limit(10)
   *   .select({ id: true, email: true })
   *   .execute();
   * // → Array<{ id: string; email: string }>
   * ```
   */
  query(): import("./builder.js").FluentQuery<T, undefined>;
}

// ─── Convenience type aliases ─────────────────────────────────────────────────
// Match naming conventions familiar from Prisma / tRPC.

/** Convenience alias for `QueryArgs<T>`. */
export type FindManyArgs<T> = QueryArgs<T>;
/** Convenience alias for `Filter<T>` — the `where` clause shape. */
export type Where<T> = Filter<T>;
/** Convenience alias for `SelectFields<T>`. */
export type Select<T> = SelectFields<T>;
/** Convenience alias for `OrderBy<T>`. */
export type OrderArgs<T> = OrderBy<T>;

// ─── Module-augmentation interfaces ──────────────────────────────────────────
// These are empty on purpose — they are augmented by the generated SDK file.

/** Augment via the generated SDK file to get typed table clients. */
// eslint-disable-next-line @typescript-eslint/no-empty-interface
export interface FluxbaseDB {}

/** Augment via the generated SDK file to get typed function calls. */
// eslint-disable-next-line @typescript-eslint/no-empty-interface
export interface FluxbaseFunctions {}

// ─── Storage client ───────────────────────────────────────────────────────────

export interface FluxbaseStorage {
  /** Upload a file and return its metadata. */
  upload(file: File | Blob, path: string): Promise<FluxFile>;
  /** Download a file by key as a Blob. */
  download(key: string): Promise<Blob>;
  /** Delete a file by key. */
  delete(key: string): Promise<void>;
  /** List files, optionally filtered by prefix. */
  list(prefix?: string): Promise<FluxFile[]>;
  /** Get a temporary signed URL for a file. */
  getUrl(key: string): Promise<string>;
}

// ─── Client ───────────────────────────────────────────────────────────────────

/** The unified Fluxbase client object. */
export interface FluxbaseClient {
  /** Typed database gateway — one entry per table. */
  db: FluxbaseDB;
  /** Typed function gateway — one entry per deployed function. */
  functions: FluxbaseFunctions;
  /** File storage operations. */
  storage: FluxbaseStorage;
}

/** Options passed to `createClient`. */
export interface ClientOptions {
  /**
   * Your Fluxbase Gateway URL.
   * Example: `"https://gateway.fluxbase.co"`
   */
  url: string;
  /** API key (or JWT bearer token). */
  apiKey: string;
  /** Optional project ID forwarded as `X-Fluxbase-Project`. */
  projectId?: string;
  /** Optional tenant ID forwarded as `X-Fluxbase-Tenant`. */
  tenantId?: string;
}
