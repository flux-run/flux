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

// ─── Table client ─────────────────────────────────────────────────────────────

/**
 * Typed table client interface.
 *
 * `T`       — full row type (all columns)
 * `TInsert` — insert payload (typically Omit<T, auto fields>)
 * `TUpdate` — update payload (typically Partial<TInsert>)
 */
export interface TableClient<T, TInsert = Partial<T>, TUpdate = Partial<T>> {
  /** Return multiple rows matching `args`. */
  findMany(args?: QueryArgs<T>): Promise<T[]>;
  /** Return the first matching row or null. */
  findOne(args?: QueryArgs<T>): Promise<T | null>;
  /** Insert one or many rows; returns the inserted rows. */
  insert(data: TInsert | TInsert[]): Promise<T[]>;
  /** Update rows matching `where`; returns the updated rows. */
  update(where: Filter<T>, data: TUpdate): Promise<T[]>;
  /** Delete rows matching `where`. */
  delete(where: Filter<T>): Promise<{ deleted: number }>;
  /** Count rows matching `where`. */
  count(args?: Pick<QueryArgs<T>, "where">): Promise<number>;
}

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
