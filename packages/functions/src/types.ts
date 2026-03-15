/**
 * Type definitions for the @flux/functions SDK.
 *
 * This file mirrors the ctx surface exposed by the Flux runtime (both V8/Deno
 * and WASM execution paths). Every method listed here has a corresponding op
 * registered in runtime/src/engine/executor.rs (V8) or wasm_executor.rs (WASM).
 */

/** Zod-like schema interface — compatible with Zod but not strictly requiring it */
export interface Schema<T = unknown> {
  parse(data: unknown): T;
  safeParse(
    data: unknown,
  ): { success: true; data: T } | { success: false; error: unknown };
  _type?: T;
}

/** Runtime secrets accessor */
export interface FluxSecrets {
  /** Get a secret value by key. Returns null if not set. */
  get(key: string): string | null;
}

/**
 * Queue — enqueue an async job for another deployed function.
 *
 * Jobs are durable: they survive runtime restarts and are retried on failure.
 * Every push is recorded and visible in `flux trace`.
 *
 * @example
 * await ctx.queue.push("send_welcome_email", { userId: user.id });
 *
 * @example — delayed job
 * await ctx.queue.push("charge_subscription", { planId }, { delay: "24h" });
 *
 * @example — idempotent job (safe to push multiple times)
 * await ctx.queue.push("sync_crm", { contactId }, { idempotencyKey: contactId });
 */
export interface FluxQueue {
  /**
   * Enqueue a job to run `functionName` asynchronously with `payload`.
   *
   * @param functionName  Name of the deployed Flux function to invoke.
   * @param payload       JSON-serialisable input for the target function.
   * @param opts          Optional: delay ("5m", "1h", "24h") and idempotencyKey.
   */
  push(
    functionName: string,
    payload: unknown,
    opts?: { delay?: string; idempotencyKey?: string },
  ): Promise<{ jobId: string }>;
}

/**
 * Cross-function invocation — call another deployed Flux function in-process.
 *
 * The callee runs synchronously within the same request context. The call is
 * recorded as a child span so it appears in `flux trace` as a nested execution.
 *
 * @example
 * const result = await ctx.function.invoke("validate_user", { userId: input.id });
 */
export interface FluxFunction {
  /**
   * Invoke another deployed function by name.
   *
   * @param name     Deployed function name (same as in flux.toml / gateway routes).
   * @param payload  JSON-serialisable input.
   * @returns        The function's return value.
   */
  invoke(name: string, payload: unknown): Promise<unknown>;
}

/**
 * SSRF-protected HTTP client.
 *
 * Blocks RFC1918 (10.x, 172.16–31.x, 192.168.x), loopback (127.x, ::1),
 * link-local (169.254.x), and cloud metadata endpoints. Strips internal
 * Flux service headers from outbound requests to prevent privilege escalation.
 *
 * Every fetch is recorded and visible in `flux trace`.
 *
 * @example
 * const res = await ctx.fetch("https://api.stripe.com/v1/charges", {
 *   method:  "POST",
 *   headers: { Authorization: `Bearer ${ctx.secrets.get("STRIPE_KEY")}` },
 *   body:    JSON.stringify({ amount: 2000, currency: "usd" }),
 * });
 * const data = await res.json();
 */
export interface FluxFetch {
  (url: string, options?: {
    method?:  string;
    headers?: Record<string, string>;
    body?:    string;
  }): Promise<{
    status:  number;
    ok:      boolean;
    headers: Record<string, string>;
    /** Response body as text */
    text():  Promise<string>;
    /** Response body parsed as JSON */
    json():  Promise<unknown>;
  }>;
}

/**
 * Tools — 900+ app integrations powered by Flux.
 *
 * Every tool call is automatically traced:
 *   ▸ tool:slack.send_message  45ms
 *
 * Setup: flux secrets set FLUX_COMPOSIO_KEY <key>
 *
 * @example
 * await ctx.tools.run("slack.send_message", {
 *   channel: "#ops",
 *   text: "New user signed up"
 * });
 */
export interface FluxTools {
  /**
   * Execute a tool by name.
   *
   * Tool names follow the format: "{app}.{action}"
   *
   * Examples: "slack.send_message", "github.create_issue", "gmail.send_email",
   *           "notion.create_page", "linear.create_issue", "stripe.create_customer"
   */
  run(toolName: string, input: Record<string, unknown>): Promise<Record<string, unknown>>;
}

/**
 * Workflow — sequential or parallel step chains.
 *
 * Each step receives the full ctx and a map of previous step outputs.
 *
 * @example
 * await ctx.workflow.run([
 *   { name: "create_user",   fn: async (ctx, prev) => createUser(ctx.payload) },
 *   { name: "notify_slack",  fn: async (ctx, prev) => ctx.tools.run("slack.send_message", { channel: "#ops", text: `User ${prev.create_user.email} signed up` }) },
 * ])
 */
export interface FluxWorkflow {
  /** Run steps sequentially; each step receives ctx and all previous outputs. */
  run(
    steps: Array<{
      name: string;
      fn: (ctx: FluxContext, previous: Record<string, unknown>) => Promise<unknown>;
    }>,
    options?: { continueOnError?: boolean }
  ): Promise<Record<string, unknown>>;

  /** Run steps concurrently; each step receives only ctx. */
  parallel(
    steps: Array<{
      name: string;
      fn: (ctx: FluxContext) => Promise<unknown>;
    }>
  ): Promise<Record<string, unknown>>;
}

/**
 * Typed database client.
 *
 * Two access patterns:
 *
 * 1. **Raw SQL** — `ctx.db.query(sql, params)` — full control, all SQL features.
 *    Use this for joins, aggregates, CTEs, and anything the ORM can't express.
 *
 * 2. **ORM-style** — `ctx.db.<table>.find(where)` — type-safe, auto-generated
 *    from your schema after `flux generate`. Import the generated context type
 *    from `.flux/types.d.ts` to get full per-table types.
 *
 * Every query is intercepted by the data-engine, mutation-recorded, and visible
 * in `flux trace` as a `db_query` span.
 *
 * @example — raw SQL
 * const rows = await ctx.db.query(
 *   "SELECT * FROM orders WHERE user_id = $1 AND status = $2",
 *   [ctx.user.id, "pending"]
 * );
 *
 * @example — ORM (requires generated types)
 * const user = await ctx.db.users.findOne({ id: input.userId });
 */
export interface FluxDb {
  /**
   * Execute a raw SQL query through the data-engine.
   * Parameters are passed as positional `$1`, `$2`, … placeholders.
   * Returns an array of rows for SELECT/WITH; `{ rows_affected: N }` for DML.
   */
  query(sql: string, params?: unknown[]): Promise<unknown[]>;

  /**
   * Alias for `query` — execute a raw SQL statement.
   * Useful when you want to distinguish reads from writes in your code.
   */
  execute(sql: string, params?: unknown[]): Promise<unknown[]>;

  /** ORM-style table accessor — full types available from `.flux/types.d.ts` */
  [table: string]: {
    find(where?: Record<string, unknown>): Promise<unknown[]>;
    findOne(where?: Record<string, unknown>): Promise<unknown | null>;
    insert(data: Record<string, unknown>): Promise<unknown>;
    update(where: Record<string, unknown>, data: Record<string, unknown>): Promise<unknown[]>;
    delete(where: Record<string, unknown>): Promise<void>;
  } | FluxDb["query"] | FluxDb["execute"];
}

/** Context object passed to every function handler */
export interface FluxContext {
  /** The raw incoming payload (pre-validation) */
  payload: unknown;
  /** Resolved secrets for this tenant/project */
  secrets: FluxSecrets;
  /** Environment variable map (same as secrets in MVP) */
  env: Record<string, string>;
  /** Emit a structured log line visible in `flux logs` and `flux trace` */
  log(message: string, level?: "info" | "warn" | "error"): void;

  /**
   * Database — raw SQL and ORM-style access, both mutation-recorded.
   * See {@link FluxDb} for the full API.
   */
  db: FluxDb;

  /**
   * Queue — enqueue async jobs for other functions.
   * Jobs are durable and retried on failure.
   */
  queue: FluxQueue;

  /**
   * Cross-function invocation.
   * Calls run synchronously in the current request context and appear as
   * child spans in `flux trace`.
   */
  function: FluxFunction;

  /**
   * SSRF-protected HTTP client.
   * Blocks private IPs and cloud metadata endpoints; strips internal headers.
   * Every call appears as an `http_fetch` span in `flux trace`.
   */
  fetch: FluxFetch;

  /**
   * Suspend the function for `ms` milliseconds without blocking the event loop.
   * Other concurrent requests continue executing during the sleep.
   * Recorded as a `sleep` span in `flux trace`.
   *
   * @example
   * await ctx.sleep(2000); // sleep 2 seconds
   */
  sleep(ms: number): Promise<void>;

  /**
   * Generate a deterministic UUID v4.
   * Seeded per-request so `flux replay` produces identical values.
   * Use this instead of `crypto.randomUUID()` when you need replay-safe IDs.
   */
  uuid(): string;

  /**
   * Generate a deterministic nanoid string.
   * Seeded per-request — replay-safe. Default size is 21 characters.
   *
   * @param size  Character count (default: 21)
   */
  nanoid(size?: number): string;

  /**
   * Tools — call 900+ external apps from within your function.
   * Each call is automatically traced and visible in `flux trace`.
   */
  tools: FluxTools;

  /**
   * Workflow — run named steps sequentially or in parallel.
   * Each step is a JS function that can call ctx.tools, ctx.log, etc.
   */
  workflow: FluxWorkflow;
}

/** Arguments to the handler function */
export interface HandlerArgs<TInput = unknown> {
  input: TInput;
  ctx: FluxContext;
}

/** The options passed to defineFunction() */
export interface DefineFunctionOptions<TInput = unknown, TOutput = unknown> {
  /** Function name (used for display and routing) */
  name: string;
  /** Optional description shown in the dashboard and workflow builder */
  description?: string;
  /** Zod schema for validating and typing the input payload */
  input?: Schema<TInput>;
  /** Zod schema for validating and typing the return value */
  output?: Schema<TOutput>;
  /** The async handler function */
  handler: (args: HandlerArgs<TInput>) => Promise<TOutput>;
}

/** The standardized function definition object returned by defineFunction() */
export interface FunctionDefinition<TInput = unknown, TOutput = unknown> {
  /** Marker so the runtime can detect a proper framework-wrapped function */
  readonly __flux: true;
  /** Function metadata */
  readonly metadata: {
    name: string;
    description?: string;
    /** JSON Schema representation of the input Zod schema (if provided) */
    input_schema: Record<string, unknown> | null;
    /** JSON Schema representation of the output Zod schema (if provided) */
    output_schema: Record<string, unknown> | null;
  };
  /**
   * Main entry point called by the runtime.
   * Validates input, runs handler, validates output.
   */
  execute(payload: unknown, context: FluxContext): Promise<TOutput>;
}
