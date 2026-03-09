/**
 * @fluxbase/sdk
 *
 * The unified Fluxbase client for TypeScript/JavaScript applications.
 *
 * Quick start:
 *
 * ```ts
 * import { createClient } from "@fluxbase/sdk";
 * // Import the auto-generated types (regenerate with GET /sdk/typescript):
 * import "./fluxbase.generated";
 *
 * const flux = createClient({
 *   url:       "https://gateway.fluxbase.co",
 *   apiKey:    "your-api-key",
 *   projectId: "proj_abc123",
 *   tenantId:  "tenant_xyz",
 * });
 *
 * // Type-safe database queries
 * const users = await flux.db.users.findMany({
 *   where:  { active: { eq: true } },
 *   select: { id: true, email: true, posts: { select: { id: true, title: true } } },
 *   limit:  20,
 * });
 *
 * // Insert
 * const [newUser] = await flux.db.users.insert({ email: "a@b.com" });
 *
 * // Type-safe function calls
 * const result = await flux.functions.send_email({
 *   to:      "user@example.com",
 *   subject: "Welcome",
 *   body:    "Hello!",
 * });
 *
 * // Storage
 * const file = await flux.storage.upload(blob, "avatars/user-1.png");
 * ```
 */

import type {
  ClientOptions,
  FluxbaseClient,
  FluxbaseDB,
  FluxbaseFunctions,
  FluxbaseStorage,
  FluxFile,
} from "./types.js";

import { buildTableClient } from "./query.js";

// ─── Public type re-exports ───────────────────────────────────────────────────

export type {
  ClientOptions,
  FluxbaseClient,
  FluxbaseDB,
  FluxbaseFunctions,
  FluxbaseStorage,
  FluxFile,
  TableClient,
  QueryArgs,
  Filter,
  FilterOp,
  OrderBy,
  SelectFields,
  // Phase 5: select result inference
  SelectResult,
  // Nested insert helpers
  Connect,
  // Convenience aliases (Prisma-familiar naming)
  FindManyArgs,
  Where,
  Select,
  OrderArgs,
} from "./types.js";

// ─── Fluent query builder ─────────────────────────────────────────────────────
export { FluentQuery } from "./builder.js";
export type { FluentQueryBuilder } from "./builder.js";

// ─── createClient ─────────────────────────────────────────────────────────────

/**
 * Create a Fluxbase client bound to `options.url`.
 *
 * The returned object exposes three namespaces:
 *   - `flux.db`        — typed table operations (CRUD + nested selects)
 *   - `flux.functions` — typed function invocations (RPC)
 *   - `flux.storage`   — file upload / download / management
 *
 * TypeScript types for `.db` and `.functions` are provided by the generated
 * SDK file (`GET /sdk/typescript`), which augments the `FluxbaseDB` and
 * `FluxbaseFunctions` interfaces via declaration merging.
 */
export function createClient(options: ClientOptions): FluxbaseClient {
  const { url, apiKey, projectId, tenantId } = options;

  // Base headers attached to every request.
  const baseHeaders: Record<string, string> = {
    "Content-Type": "application/json",
    Authorization:  `Bearer ${apiKey}`,
    ...(tenantId  ? { "X-Fluxbase-Tenant":  tenantId  } : {}),
    ...(projectId ? { "X-Fluxbase-Project": projectId } : {}),
  };

  // ── Core HTTP fetcher ─────────────────────────────────────────────────────
  async function fetcher(path: string, init?: RequestInit): Promise<unknown> {
    const merged: RequestInit = {
      ...init,
      headers: {
        ...baseHeaders,
        ...(init?.headers as Record<string, string> | undefined ?? {}),
      },
    };

    const res = await fetch(`${url}${path}`, merged);

    if (!res.ok) {
      const err = await res
        .json()
        .catch(() => ({ error: `HTTP ${res.status}` })) as { error?: string };
      throw new Error(err.error ?? `HTTP ${res.status}`);
    }

    const text = await res.text();
    if (!text) return {};
    return JSON.parse(text) as unknown;
  }

  // ── db proxy ─────────────────────────────────────────────────────────────
  // Uses a Proxy so accessing any property (e.g. `flux.db.users`) returns a
  // fully-functional TableClient at runtime. TypeScript narrows the type via
  // the module augmentation in the generated SDK file.
  const db = new Proxy({} as FluxbaseDB, {
    get(_target, table: string | symbol): unknown {
      if (typeof table !== "string") return undefined;
      return buildTableClient(table, fetcher);
    },
  });

  // ── functions proxy ───────────────────────────────────────────────────────
  // Accessing `flux.functions.send_email` returns a callable that POSTs to
  // `/<function-name>` on the Gateway.
  const functions = new Proxy({} as FluxbaseFunctions, {
    get(_target, name: string | symbol): unknown {
      if (typeof name !== "string") return undefined;
      return (input: unknown) =>
        fetcher(`/${name}`, {
          method: "POST",
          body:   JSON.stringify(input ?? {}),
        });
    },
  });

  // ── storage ───────────────────────────────────────────────────────────────
  const storage: FluxbaseStorage = {
    async upload(file: File | Blob, path: string): Promise<FluxFile> {
      // Step 1 — ask the Gateway for a pre-signed PUT URL.
      const signed = (await fetcher("/files/upload-url", {
        method: "POST",
        body:   JSON.stringify({
          path,
          content_type: (file as File).type || "application/octet-stream",
          size:         file.size,
        }),
      })) as { upload_url: string; key: string };

      // Step 2 — PUT the file directly to cloud storage.
      const mime = (file as File).type || "application/octet-stream";
      await fetch(signed.upload_url, {
        method:  "PUT",
        body:    file,
        headers: { "Content-Type": mime },
      });

      return {
        url:       signed.upload_url.split("?")[0],
        key:       signed.key,
        size:      file.size,
        mime_type: mime,
      };
    },

    async download(key: string): Promise<Blob> {
      const res = await fetch(`${url}/files/${key}`, {
        headers: baseHeaders,
      });
      if (!res.ok) throw new Error(`HTTP ${res.status}`);
      return res.blob();
    },

    async delete(key: string): Promise<void> {
      await fetcher(`/files/${key}`, { method: "DELETE" });
    },

    async list(prefix?: string): Promise<FluxFile[]> {
      const q = prefix ? `?prefix=${encodeURIComponent(prefix)}` : "";
      return (await fetcher(`/files${q}`)) as FluxFile[];
    },

    async getUrl(key: string): Promise<string> {
      const res = (await fetcher(`/files/${key}/url`)) as { url: string };
      return res.url;
    },
  };

  return { db, functions, storage };
}
