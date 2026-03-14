/**
 * @fluxbase/client — Fluxbase client SDK
 *
 * Call your deployed Flux functions from any TypeScript or JavaScript project.
 *
 * @example
 * ```ts
 * import { createClient } from "@fluxbase/client";
 *
 * const flux = createClient({
 *   url:   "https://your-gateway.example.com",
 *   token: process.env.FLUX_TOKEN,
 * });
 *
 * // Call a deployed function
 * const result = await flux.call("send-welcome-email", { userId: "123" });
 *
 * // Type-safe with generics
 * const order = await flux.call<{ orderId: string }>("create-order", {
 *   items: [{ sku: "A1", qty: 2 }],
 * });
 * ```
 */

// ── Types ─────────────────────────────────────────────────────────────────────

/** Options for creating a Flux client */
export interface FluxClientOptions {
  /**
   * Base URL of your Flux gateway.
   * Example: "https://gateway.example.com" or "http://localhost:4000" for local dev.
   */
  url: string;
  /**
   * API token for authenticating requests.
   * Generate one with: `flux token create`
   */
  token?: string;
  /**
   * Default request timeout in milliseconds. Defaults to 30 000 ms (30 s).
   */
  timeoutMs?: number;
}

/** A raw Flux API error returned from the gateway */
export interface FluxApiError {
  error:   string;
  message: string;
  code:    number;
}

/** Thrown when a function call returns a non-2xx response */
export class FluxError extends Error {
  readonly status:  number;
  readonly code:    string;
  readonly details: FluxApiError;

  constructor(details: FluxApiError) {
    super(details.message);
    this.name    = "FluxError";
    this.status  = details.code;
    this.code    = details.error;
    this.details = details;
  }
}

/** The Flux client */
export interface FluxClient {
  /**
   * Call a deployed Flux function by name or ID.
   *
   * @param functionName  The function's slug or UUID
   * @param input         JSON-serialisable input payload
   * @param options       Per-call overrides (timeout, headers)
   * @returns             The function's return value
   * @throws {FluxError}  If the gateway returns a non-2xx response
   */
  call<TOutput = unknown>(
    functionName: string,
    input?:       unknown,
    options?:     CallOptions,
  ): Promise<TOutput>;
}

/** Per-call overrides */
export interface CallOptions {
  /** Override timeout for this call (ms) */
  timeoutMs?: number;
  /** Additional headers to merge into the request */
  headers?: Record<string, string>;
}

// ── createClient ──────────────────────────────────────────────────────────────

/**
 * Create a Flux client instance.
 *
 * @example
 * ```ts
 * const flux = createClient({ url: "https://gw.example.com", token: "tok_..." });
 * const result = await flux.call("my-function", { hello: "world" });
 * ```
 */
export function createClient(options: FluxClientOptions): FluxClient {
  const { url, token, timeoutMs: defaultTimeout = 30_000 } = options;
  const baseUrl = url.replace(/\/$/, "");

  async function call<TOutput = unknown>(
    functionName: string,
    input:        unknown = null,
    callOpts:     CallOptions = {},
  ): Promise<TOutput> {
    const timeout = callOpts.timeoutMs ?? defaultTimeout;

    const headers: Record<string, string> = {
      "Content-Type": "application/json",
      ...callOpts.headers,
    };
    if (token) {
      headers["Authorization"] = `Bearer ${token}`;
    }

    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeout);

    let response: Response;
    try {
      response = await fetch(`${baseUrl}/invoke/${functionName}`, {
        method:  "POST",
        headers,
        body:    JSON.stringify(input),
        signal:  controller.signal,
      });
    } catch (err: unknown) {
      clearTimeout(timer);
      if (err instanceof Error && err.name === "AbortError") {
        throw new FluxError({
          error:   "TIMEOUT",
          message: `Request to '${functionName}' timed out after ${timeout}ms`,
          code:    504,
        });
      }
      throw err;
    } finally {
      clearTimeout(timer);
    }

    if (!response.ok) {
      let errorBody: FluxApiError;
      try {
        errorBody = await response.json() as FluxApiError;
      } catch {
        errorBody = {
          error:   "FUNCTION_ERROR",
          message: `Function '${functionName}' returned HTTP ${response.status}`,
          code:    response.status,
        };
      }
      throw new FluxError(errorBody);
    }

    return response.json() as Promise<TOutput>;
  }

  return { call };
}
