/**
 * flux-schema-compiler.ts
 *
 * Deno script run by `flux db push` to compile *.schema.ts files.
 */

// ── Arg parsing ───────────────────────────────────────────────────────────────

const schemasDir = Deno.args[0] ?? "./schemas";

// ── Find all *.schema.ts files ────────────────────────────────────────────────

async function findSchemaFiles(dir: string): Promise<string[]> {
  const files: string[] = [];
  try {
    for await (const entry of Deno.readDir(dir)) {
      const path = `${dir}/${entry.name}`;
      if (entry.isDirectory) {
        files.push(...await findSchemaFiles(path));
      } else if (entry.name.endsWith(".schema.ts") && !entry.name.startsWith("_")) {
        files.push(path);
      }
    }
  } catch {
    // dir doesn't exist — return empty
  }
  return files.sort();
}

// ── Rule compiler ─────────────────────────────────────────────────────────────
// Converts TypeScript predicate functions to RuleExpr AST JSON.
// We use a simple pattern-matching approach on the function source string,
// delegating to a full TS→AST parser for complex expressions.

function compileRule(fn: unknown, _name: string): unknown {
  if (fn === null || fn === undefined) return null;

  // Already a compiled RuleExpr object (user passed raw AST)
  if (typeof fn === "object" && "t" in (fn as object)) return fn;

  if (typeof fn !== "function") return null;

  // Call the function with a proxy that records the access pattern
  try {
    const result = tryCompileRuleFn(fn as (...args: unknown[]) => unknown);
    return result;
  } catch {
    // Fall back to WASM path marker
    return { t: "wasm_fn", source: fn.toString() };
  }
}

// Proxy-based rule compiler: calls the function with proxy objects
// that record field accesses, then reconstructs the AST.

function tryCompileRuleFn(fn: (...args: unknown[]) => unknown): unknown {
  const accesses: { type: string; path: string[] }[] = [];

  function makeProxy(prefix: string[]): unknown {
    return new Proxy({} as Record<string, unknown>, {
      get(_, prop) {
        if (typeof prop !== "string") return undefined;
        const path = [...prefix, prop];
        if (prop === "id" || prop === "role" || prop === "email") {
          accesses.push({ type: "ctx", path });
        }
        return makeProxy(path);
      },
    });
  }

  const ctx  = { user: makeProxy(["user"]) };
  const row  = makeProxy(["row"]);
  const prev = makeProxy(["prev"]);
  const input = makeProxy(["input"]);

  // We can't actually evaluate the boolean expression via proxies
  // without a full AST parser. For now, emit source for server-side parsing.
  const _ = [ctx, row, prev, input]; // suppress unused warning
  return { t: "wasm_fn", source: fn.toString() };
}

// ── Transform compiler ────────────────────────────────────────────────────────

function compileHook(fn: unknown): unknown {
  if (fn === null || fn === undefined) return null;
  if (typeof fn === "object" && "kind" in (fn as object)) return fn;
  if (typeof fn !== "function") return null;
  return { kind: "wasm_fn", source: fn.toString() };
}

// ── On-spec compiler ──────────────────────────────────────────────────────────

function compileOn(spec: unknown): unknown {
  if (spec === null || spec === undefined) return null;
  if (Array.isArray(spec)) {
    return { t: "static", functions: spec };
  }
  if (typeof spec === "function") {
    return { t: "wasm_fn", source: (spec as (...a: unknown[]) => unknown).toString() };
  }
  return spec;
}

// ── Main ──────────────────────────────────────────────────────────────────────

const files = await findSchemaFiles(schemasDir);

for (const file of files) {
  try {
    const absPath = `${Deno.cwd()}/${file}`.replace(/\/+/g, "/");
    const url = absPath.startsWith("/") ? `file://${absPath}` : `file:///${absPath}`;
    const mod = await import(url);
    const schema = mod.default ?? mod.schema;

    if (!schema) {
      console.error(JSON.stringify({ error: `No default export in ${file}` }));
      continue;
    }

    const { name, table } = schema;
    const tableName = table ?? name;
    if (!tableName) {
      console.error(JSON.stringify({ error: `No table name in ${file}` }));
      continue;
    }

    // Compile rules
    const rawRules = schema.rules ?? {};
    const rules: Record<string, unknown> = {};
    for (const [op, fn] of Object.entries(rawRules)) {
      const compiled = compileRule(fn, op);
      if (compiled !== null) rules[op] = compiled;
    }

    // Compile before/after hooks
    const hooks: Record<string, unknown> = {};
    for (const [event, fn] of Object.entries(schema.before ?? {})) {
      const compiled = compileHook(fn);
      if (compiled !== null) hooks[`before_${event}`] = compiled;
    }
    for (const [event, fn] of Object.entries(schema.after ?? {})) {
      const compiled = compileHook(fn);
      if (compiled !== null) hooks[`after_${event}`] = compiled;
    }

    // Compile on-handlers
    const onSpecs: Record<string, unknown> = {};
    for (const [event, spec] of Object.entries(schema.on ?? {})) {
      const compiled = compileOn(spec);
      if (compiled !== null) onSpecs[event] = compiled;
    }

    const manifest = {
      table:        tableName,
      file:         file,
      columns:      schema.columns ?? {},
      indexes:      schema.indexes ?? [],
      foreign_keys: schema.foreign_keys ?? [],
      rules,
      hooks,
      on:           onSpecs,
    };

    console.log(JSON.stringify(manifest));
  } catch (err) {
    const msg = err instanceof Error ? err.message : String(err);
    console.error(JSON.stringify({ error: `Failed to compile ${file}: ${msg}` }));
    Deno.exit(1);
  }
}
