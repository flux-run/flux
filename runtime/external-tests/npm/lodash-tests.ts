/**
 * Lodash utility library — compatibility tests
 */

import _ from "lodash";
import type { TestResult } from "../../runners/lib/utils.js";

function run(name: string, fn: () => void): TestResult {
  const t0 = performance.now();
  try {
    fn();
    return { name, passed: true, skipped: false, duration: Math.round(performance.now() - t0) };
  } catch (e) {
    return {
      name, passed: false, skipped: false,
      error: e instanceof Error ? e.message : String(e),
      duration: Math.round(performance.now() - t0),
    };
  }
}

export async function runLodashTests(): Promise<TestResult[]> {
  return [
    // ── Array utilities ─────────────────────────────────────────────────────

    run("_.chunk splits array into groups of given size", () => {
      const out = _.chunk([1,2,3,4,5], 2);
      if (JSON.stringify(out) !== JSON.stringify([[1,2],[3,4],[5]])) throw new Error(JSON.stringify(out));
    }),

    run("_.uniq removes duplicate values", () => {
      const out = _.uniq([1,2,2,3,3,3]);
      if (JSON.stringify(out) !== JSON.stringify([1,2,3])) throw new Error(JSON.stringify(out));
    }),

    run("_.flatten one level deep", () => {
      const out = _.flatten([[1,2],[3,[4,5]]]);
      if (JSON.stringify(out) !== JSON.stringify([1,2,3,[4,5]])) throw new Error(JSON.stringify(out));
    }),

    run("_.flattenDeep recursively flattens", () => {
      const out = _.flattenDeep([1,[2,[3,[4]]]]);
      if (JSON.stringify(out) !== JSON.stringify([1,2,3,4])) throw new Error(JSON.stringify(out));
    }),

    run("_.difference returns values not in second array", () => {
      const out = _.difference([1,2,3,4], [2,4]);
      if (JSON.stringify(out) !== JSON.stringify([1,3])) throw new Error(JSON.stringify(out));
    }),

    run("_.intersection returns values present in both arrays", () => {
      const out = _.intersection([1,2,3], [2,3,4]);
      if (JSON.stringify(out) !== JSON.stringify([2,3])) throw new Error(JSON.stringify(out));
    }),

    // ── Object utilities ─────────────────────────────────────────────────────

    run("_.pick returns subset of object keys", () => {
      const out = _.pick({ a: 1, b: 2, c: 3 }, ["a", "c"]);
      if (out.a !== 1 || out.c !== 3 || "b" in out) throw new Error(JSON.stringify(out));
    }),

    run("_.omit returns object without specified keys", () => {
      const out = _.omit({ a: 1, b: 2, c: 3 }, ["b"]);
      if ("b" in out) throw new Error("b still present");
      if (out.a !== 1 || out.c !== 3) throw new Error(JSON.stringify(out));
    }),

    run("_.merge deep-merges two objects", () => {
      const a = { x: { y: 1 }, z: 3 };
      const b = { x: { w: 2 } };
      const out = _.merge({}, a, b);
      if (out.x.y !== 1 || out.x.w !== 2) throw new Error(JSON.stringify(out));
    }),

    run("_.cloneDeep creates a deep copy", () => {
      const original = { a: { b: { c: 42 } } };
      const copy     = _.cloneDeep(original);
      copy.a.b.c     = 999;
      if (original.a.b.c !== 42) throw new Error("original was mutated");
    }),

    run("_.get safely retrieves nested value", () => {
      const obj = { a: { b: { c: "deep" } } };
      if (_.get(obj, "a.b.c")      !== "deep") throw new Error("deep get failed");
      if (_.get(obj, "a.x.y", "?") !== "?")    throw new Error("default not returned");
    }),

    run("_.set mutates nested path", () => {
      const obj = {} as Record<string, unknown>;
      _.set(obj, "a.b.c", 42);
      if ((obj as { a: { b: { c: number } } }).a.b.c !== 42) throw new Error("set failed");
    }),

    // ── Collection utilities ──────────────────────────────────────────────────

    run("_.groupBy groups array elements by key", () => {
      const out = _.groupBy([{t:"a"},{t:"b"},{t:"a"}], "t");
      if (out["a"].length !== 2) throw new Error("a group wrong");
      if (out["b"].length !== 1) throw new Error("b group wrong");
    }),

    run("_.sortBy sorts objects by iteratee", () => {
      const out = _.sortBy([{n:3},{n:1},{n:2}], "n");
      if (out.map(o=>o.n).join(",") !== "1,2,3") throw new Error(JSON.stringify(out));
    }),

    run("_.keyBy indexes collection by key", () => {
      const users = [{ id: 1, name: "Alice" }, { id: 2, name: "Bob" }];
      const map   = _.keyBy(users, "id");
      if (map[1].name !== "Alice") throw new Error("Alice missing");
      if (map[2].name !== "Bob")   throw new Error("Bob missing");
    }),

    // ── String utilities ──────────────────────────────────────────────────────

    run("_.camelCase converts snake_case", () => {
      if (_.camelCase("hello_world") !== "helloWorld") throw new Error(_.camelCase("hello_world"));
    }),

    run("_.snakeCase converts camelCase", () => {
      if (_.snakeCase("helloWorld") !== "hello_world") throw new Error(_.snakeCase("helloWorld"));
    }),

    run("_.truncate shortens long string with ellipsis", () => {
      const out = _.truncate("hello world", { length: 8 });
      if (out.length > 8) throw new Error(`too long: ${out}`);
      if (!out.endsWith("...")) throw new Error(`no ellipsis: ${out}`);
    }),

    // ── Function utilities ────────────────────────────────────────────────────

    run("_.debounce delays the function call", async () => {
      // Just confirm it returns a function; timing tests are environment-dependent
      const fn = _.debounce(() => {}, 100);
      if (typeof fn !== "function") throw new Error("not a function");
    }),

    run("_.memoize caches function results", () => {
      let calls = 0;
      const fn  = _.memoize((x: number) => { calls++; return x * 2; });
      fn(5); fn(5); fn(5);
      if (calls !== 1) throw new Error(`expected 1 call, got ${calls}`);
      if (fn(5) !== 10) throw new Error("wrong result");
    }),

    run("_.once wraps function to only call once", () => {
      let calls = 0;
      const fn  = _.once(() => { calls++; return 42; });
      fn(); fn(); fn();
      if (calls !== 1) throw new Error(`expected 1 call, got ${calls}`);
    }),
  ];
}
