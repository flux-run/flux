/**
 * Type definitions for the @fluxbase/functions SDK.
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
 * Tools — 900+ app integrations powered by Fluxbase.
 *
 * Every tool call is automatically traced:
 *   ▸ tool:slack.send_message  45ms
 *
 * Setup: flux secrets set FLUXBASE_COMPOSIO_KEY <key>
 *
 * @example
 * await ctx.tools.run("slack.send_message", {
 *   channel: "#ops",
 *   text: "New user signed up"
 * });
 *
 * @example
 * await ctx.tools.run("github.create_issue", {
 *   owner: "my-org",
 *   repo:  "my-repo",
 *   title: "Bug reported by user",
 *   body:  `User ${input.email} reported: ${input.message}`,
 * });
 */
export interface FluxTools {
  /**
   * Execute a tool by name.
   *
   * Tool names follow the format: "{app}.{action}"
   *
   * Examples:
   *   "slack.send_message"
   *   "github.create_issue"
   *   "gmail.send_email"
   *   "notion.create_page"
   *   "linear.create_issue"
   *   "jira.create_issue"
   *   "airtable.create_record"
   *   "stripe.create_customer"
   *
   * @param toolName  Fluxbase tool identifier
   * @param input     Tool-specific input parameters
   * @returns         Tool output data
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

/** Result returned by ctx.agent.run() */
export interface FluxAgentResult {
  /** LLM summary of what was accomplished */
  answer: string;
  /** Number of reasoning steps taken */
  steps: number;
  /** Output from the last tool call */
  output: Record<string, unknown> | null;
}

/**
 * Agent — LLM-driven autonomous tool execution.
 *
 * The agent loops: decide which tool to call → call it via ctx.tools.run() → observe result → repeat.
 * The loop ends when the LLM says the goal is done or maxSteps is reached.
 *
 * Requires FLUXBASE_LLM_KEY secret (OpenAI-compatible API key).
 * Optional: FLUXBASE_LLM_URL (default: OpenAI), FLUXBASE_LLM_MODEL (default: gpt-4o-mini).
 *
 * @example
 * const result = await ctx.agent.run({
 *   goal: "Create a Linear issue for the bug and notify #dev on Slack",
 *   tools: ["linear.create_issue", "slack.send_message"],
 *   maxSteps: 5,
 * })
 * // result.answer = "Created Linear issue LIN-123 and posted to #dev"
 */
export interface FluxAgent {
  run(options: {
    /** What to accomplish */
    goal: string;
    /** Tool names the agent is allowed to use */
    tools?: string[];
    /** Maximum reasoning iterations (default: 5) */
    maxSteps?: number;
  }): Promise<FluxAgentResult>;
}

/** Context object passed to every function handler */
export interface FluxContext {
  /** The raw incoming payload (pre-validation) */
  payload: unknown;
  /** Resolved secrets for this tenant/project */
  secrets: FluxSecrets;
  /** Environment variable map (same as secrets in MVP) */
  env: Record<string, string>;
  /** Emit a structured log line */
  log(message: string, level?: "info" | "warn" | "error"): void;
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
  /**
   * Agent — LLM-driven autonomous execution loop.
   * The agent picks tools and calls them until the goal is achieved.
   * Requires FLUXBASE_LLM_KEY secret.
   */
  agent: FluxAgent;
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
  readonly __fluxbase: true;
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
