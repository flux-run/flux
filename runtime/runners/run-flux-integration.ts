/**
 * run-flux-integration.ts
 *
 * Integration tests that compile and run the actual flux-runtime binary,
 * then send real HTTP requests to validate runtime behaviour.
 *
 * Unlike the internal test suites (trust / compat / replay / modules), which
 * run inside a Node.js process, these tests prove that the *Flux binary* is
 * working correctly end-to-end.
 *
 * Usage
 * -----
 *   npm run test:integration              # build binary (if missing) + test
 *   npm run test:integration -- --skip-build   # skip cargo build
 *   npm run test:integration -- --suite echo   # run one suite only
 *
 * Output
 * ------
 *   runtime/reports/flux-integration.json
 */

import { performance }   from "node:perf_hooks";
import { resolve }       from "node:path";
import { dirname }       from "node:path";
import { fileURLToPath } from "node:url";
import {
  ensureBinary,
  startRuntime,
  postJson,
  get,
  type RuntimeHandle,
} from "./lib/flux-binary.js";
import { TestResult, buildReport, writeReport, printSummary } from "./lib/utils.js";

const __dirname   = dirname(fileURLToPath(import.meta.url));
const HANDLERS_DIR = resolve(__dirname, "../external-tests/flux-handlers");

// Each suite gets its own port in the 3100-3199 range so suites can run
// sequentially without port conflicts when multiple are enabled.
let nextPort = 3100;
function allocatePort() { return nextPort++; }

// ---------------------------------------------------------------------------
// Assertion helper
// ---------------------------------------------------------------------------

interface AssertionContext {
  results: TestResult[];
}

function assert(
  ctx: AssertionContext,
  name: string,
  fn: () => boolean | string,
): void {
  const start = performance.now();
  let passed = false;
  let error: string | undefined;
  try {
    const res = fn();
    if (typeof res === "string") {
      passed = false;
      error  = res;
    } else {
      passed = res;
      if (!passed) error = "assertion returned false";
    }
  } catch (e) {
    passed = false;
    error  = (e as Error).message;
  }
  ctx.results.push({
    name,
    passed,
    skipped: false,
    error,
    duration: performance.now() - start,
  });
}

// ---------------------------------------------------------------------------
// Suite runner wrapper
// ---------------------------------------------------------------------------

interface Suite {
  name:    string;
  handler: string;   // filename inside HANDLERS_DIR
  run: (baseUrl: string, ctx: AssertionContext) => Promise<void>;
}

async function runSuite(suite: Suite): Promise<{ passed: number; failed: number; results: TestResult[] }> {
  const port    = allocatePort();
  const entry   = resolve(HANDLERS_DIR, suite.handler);
  const ctx: AssertionContext = { results: [] };

  let runtime: RuntimeHandle | null = null;
  try {
    runtime = await startRuntime(entry, port);
    await suite.run(runtime.baseUrl, ctx);
  } catch (err) {
    ctx.results.push({
      name:    `[suite startup] ${suite.name}`,
      passed:  false,
      skipped: false,
      error:   (err as Error).message,
      duration: 0,
    });
  } finally {
    await runtime?.stop();
  }

  const passed = ctx.results.filter((r) => r.passed).length;
  const failed = ctx.results.filter((r) => !r.passed).length;
  return { passed, failed, results: ctx.results };
}

// ---------------------------------------------------------------------------
// Suite definitions
// ---------------------------------------------------------------------------

const SUITES: Suite[] = [

  // ── 1. Echo ─────────────────────────────────────────────────────────────
  {
    name:    "echo",
    handler: "echo.js",
    async run(baseUrl, ctx) {
      {
        const r = await get(baseUrl, "/ping");
        assert(ctx, "GET /ping → 200", () => r.status === 200);
        assert(ctx, "GET /ping body has ok:true", () => (r.body as any)?.ok === true);
      }
      {
        const payload = { hello: "world", num: 42 };
        const r = await postJson(baseUrl, "/echo", payload);
        assert(ctx, "POST /echo → 200", () => r.status === 200);
        assert(ctx, "POST /echo reflects string field", () => (r.body as any)?.hello === "world");
        assert(ctx, "POST /echo reflects numeric field", () => (r.body as any)?.num === 42);
      }
      {
        const r = await postJson(baseUrl, "/echo/upper", { greeting: "hello", count: 7 });
        assert(ctx, "POST /echo/upper → 200", () => r.status === 200);
        assert(ctx, "POST /echo/upper uppercases strings", () => (r.body as any)?.greeting === "HELLO");
        assert(ctx, "POST /echo/upper passes through numbers", () => (r.body as any)?.count === 7);
      }
      {
        const res = await fetch(`${baseUrl}/echo`, {
          method:  "POST",
          headers: { "content-type": "application/json" },
          body:    "not json {{",
        });
        assert(ctx, "POST /echo with bad JSON → 400", () => res.status === 400);
      }
    },
  },

  // ── 2. JSON types ────────────────────────────────────────────────────────
  {
    name:    "json-types",
    handler: "json-types.js",
    async run(baseUrl, ctx) {
      {
        const r = await get(baseUrl, "/types/null");
        assert(ctx, "GET /types/null → value is null", () => (r.body as any)?.value === null);
      }
      {
        const r = await get(baseUrl, "/types/bool");
        assert(ctx, "GET /types/bool → value is true", () => (r.body as any)?.value === true);
      }
      {
        const r = await get(baseUrl, "/types/number");
        assert(ctx, "GET /types/number → integer 42", () => (r.body as any)?.value === 42);
        assert(ctx, "GET /types/number → float 3.14", () => Math.abs((r.body as any)?.float - 3.14) < 0.001);
      }
      {
        const r = await get(baseUrl, "/types/string");
        assert(ctx, "GET /types/string → 'hello flux'", () => (r.body as any)?.value === "hello flux");
      }
      {
        const r = await get(baseUrl, "/types/array");
        const v = (r.body as any)?.value;
        assert(ctx, "GET /types/array → array length 4", () => Array.isArray(v) && v.length === 4);
        assert(ctx, "GET /types/array → element types", () => v[0] === 1 && v[1] === "two" && v[2] === true && v[3] === null);
      }
      {
        const r = await get(baseUrl, "/types/nested");
        const o = (r.body as any)?.outer;
        assert(ctx, "GET /types/nested → deep field", () => o?.inner?.deep === "yes");
        assert(ctx, "GET /types/nested → nested array", () => Array.isArray(o?.arr));
      }
      {
        const r = await get(baseUrl, "/types/all");
        const b = r.body as any;
        assert(ctx, "GET /types/all → null field",   () => b?.null === null);
        assert(ctx, "GET /types/all → bool false",   () => b?.bool === false);
        assert(ctx, "GET /types/all → negative int", () => b?.integer === -7);
        assert(ctx, "GET /types/all → UTF-8 string", () => typeof b?.string === "string" && b.string.includes("🎉"));
      }
      {
        const r = await get(baseUrl, "/types/missing");
        assert(ctx, "GET /types/missing → 404", () => r.status === 404);
      }
    },
  },

  // ── 3. Web APIs ──────────────────────────────────────────────────────────
  {
    name:    "web-apis",
    handler: "web-apis.js",
    async run(baseUrl, ctx) {
      {
        const r = await get(baseUrl, "/web/uuid");
        assert(ctx, "GET /web/uuid → valid RFC-4122 UUID",
          () => /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i
            .test((r.body as any)?.id));
        assert(ctx, "GET /web/uuid → valid:true", () => (r.body as any)?.valid === true);
      }
      {
        const r = await get(baseUrl, "/web/date");
        assert(ctx, "GET /web/date → timestamp is number", () => typeof (r.body as any)?.timestamp === "number");
        assert(ctx, "GET /web/date → ISO string", () => typeof (r.body as any)?.iso === "string");
      }
      {
        const r = await get(baseUrl, "/web/url");
        const b = r.body as any;
        assert(ctx, "GET /web/url → host", () => b?.host === "example.com");
        assert(ctx, "GET /web/url → pathname", () => b?.pathname === "/path");
        assert(ctx, "GET /web/url → foo param", () => b?.foo === "1");
      }
      {
        const r = await get(baseUrl, "/web/url-build");
        const b = r.body as any;
        assert(ctx, "GET /web/url-build → href contains path", () => typeof b?.href === "string" && b.href.includes("/v1/users"));
        assert(ctx, "GET /web/url-build → page param", () => b?.page === "2");
        assert(ctx, "GET /web/url-build → pathname", () => b?.path === "/v1/users");
      }
      {
        const r = await get(baseUrl, "/web/math");
        const b = r.body as any;
        assert(ctx, "GET /web/math → random in [0,1)", () => b?.random_in_range === true);
        assert(ctx, "GET /web/math → floor(3.9)=3",   () => b?.floor === 3);
        assert(ctx, "GET /web/math → ceil(3.1)=4",    () => b?.ceil === 4);
        assert(ctx, "GET /web/math → abs(-7)=7",      () => b?.abs === 7);
        assert(ctx, "GET /web/math → min(5,3,8)=3",   () => b?.min === 3);
        assert(ctx, "GET /web/math → max(5,3,8)=8",   () => b?.max === 8);
        assert(ctx, "GET /web/math → 2^10=1024",      () => b?.pow === 1024);
      }
      {
        const r = await get(baseUrl, "/web/json");
        const b = r.body as any;
        assert(ctx, "GET /web/json → JSON round-trip match", () => b?.match === true);
        assert(ctx, "GET /web/json → json field is string", () => typeof b?.json === "string");
      }
    },
  },

  // ── 4. Async ops ─────────────────────────────────────────────────────────
  {
    name:    "async-ops",
    handler: "async-ops.js",
    async run(baseUrl, ctx) {
      {
        const r = await get(baseUrl, "/async/await");
        assert(ctx, "GET /async/await → result 3", () => (r.body as any)?.result === 3);
      }
      {
        const r = await get(baseUrl, "/async/promise-all");
        const results = (r.body as any)?.results;
        assert(ctx, "GET /async/promise-all → 3 items", () => Array.isArray(results) && results.length === 3);
        assert(ctx, "GET /async/promise-all → correct values",
          () => results[0] === "alpha" && results[1] === "beta" && results[2] === "gamma");
      }
      {
        const r = await get(baseUrl, "/async/promise-race");
        assert(ctx, "GET /async/promise-race → fast wins", () => (r.body as any)?.winner === "fast");
      }
      {
        const r = await get(baseUrl, "/async/microtask");
        const order = (r.body as any)?.order;
        assert(ctx, "GET /async/microtask → 2 items", () => Array.isArray(order) && order.length === 2);
        assert(ctx, "GET /async/microtask → ordering", () => order[0] === "microtask-1" && order[1] === "microtask-2");
      }
      {
        const r = await postJson(baseUrl, "/async/pipeline", { value: 5 });
        assert(ctx, "POST /async/pipeline → step1 = 10", () => (r.body as any)?.step1 === 10);
        assert(ctx, "POST /async/pipeline → step2 = 20", () => (r.body as any)?.step2 === 20);
      }
    },
  },

  // ── 5. Error handling ────────────────────────────────────────────────────
  {
    name:    "error-handling",
    handler: "error-handling.js",
    async run(baseUrl, ctx) {
      {
        const r = await get(baseUrl, "/error/not-found");
        assert(ctx, "GET /error/not-found → 404", () => r.status === 404);
        assert(ctx, "GET /error/not-found → error field", () => typeof (r.body as any)?.error === "string");
      }
      {
        const r = await get(baseUrl, "/error/bad-request");
        assert(ctx, "GET /error/bad-request → 400", () => r.status === 400);
      }
      {
        // Unhandled sync throw — runtime should return a 5xx
        const r = await get(baseUrl, "/error/sync-throw");
        assert(ctx, "GET /error/sync-throw → 5xx", () => r.status >= 500);
      }
      {
        const r = await get(baseUrl, "/error/async-reject");
        assert(ctx, "GET /error/async-reject → 5xx", () => r.status >= 500);
      }
      {
        const r = await postJson(baseUrl, "/error/missing-field", {});
        assert(ctx, "POST /error/missing-field with empty body → 422", () => r.status === 422);
      }
      {
        const r = await postJson(baseUrl, "/error/missing-field", { name: "Alice" });
        assert(ctx, "POST /error/missing-field with name → 200", () => r.status === 200);
        assert(ctx, "POST /error/missing-field with name → greeting", () =>
          (r.body as any)?.greeting === "Hello, Alice");
      }
    },
  },

];

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

const suiteArg     = process.argv.indexOf("--suite");
const SUITE_FILTER = suiteArg !== -1 ? process.argv[suiteArg + 1] : undefined;

const activeSuites = SUITE_FILTER
  ? SUITES.filter((s) => s.name.includes(SUITE_FILTER))
  : SUITES;

async function main() {
  console.log("\n╔═══════════════════════════════════════════╗");
  console.log(  "║   Flux Runtime Integration Tests          ║");
  console.log(  "╚═══════════════════════════════════════════╝\n");

  // Build (or verify) the binary before doing anything else
  try {
    ensureBinary({ quiet: false });
  } catch (err) {
    console.error(`\nFailed to ensure flux-runtime binary:\n  ${(err as Error).message}\n`);
    process.exit(1);
  }

  let totalPassed = 0;
  let totalFailed = 0;

  interface SuiteReport {
    suite:   string;
    passed:  number;
    failed:  number;
    results: TestResult[];
  }
  const allReports: SuiteReport[] = [];

  for (const suite of activeSuites) {
    process.stdout.write(`  Running: ${suite.name} … `);
    const start = performance.now();

    const { passed, failed, results } = await runSuite(suite);

    const elapsed = (performance.now() - start).toFixed(0);
    const icon    = failed === 0 ? "✓" : "✗";
    console.log(`${icon}  ${passed}/${passed + failed} passed  (${elapsed}ms)`);

    if (failed > 0) {
      for (const r of results.filter((x) => !x.passed)) {
        console.log(`    ✗ ${r.name}: ${r.error ?? "failed"}`);
      }
    }

    totalPassed += passed;
    totalFailed += failed;
    allReports.push({ suite: suite.name, passed, failed, results });
  }

  // Write report
  const totalElapsed = allReports.reduce((s, r) => s + r.results.reduce((a, x) => a + x.duration, 0), 0);
  const report = buildReport("flux-integration", allReports.flatMap((r) => r.results), totalElapsed);
  writeReport("flux-integration", report);

  // Summary banner
  console.log("\n─────────────────────────────────────────────");
  const total = totalPassed + totalFailed;
  if (totalFailed === 0) {
    console.log(`  ✓  All ${total} integration checks passed.`);
  } else {
    console.log(`  ✗  ${totalFailed}/${total} checks FAILED.`);
  }
  console.log("─────────────────────────────────────────────\n");

  if (totalFailed > 0) process.exit(1);
}

main().catch((err) => {
  console.error("Unexpected error:", err);
  process.exit(1);
});
